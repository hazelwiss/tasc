use alloc::boxed::Box;
use core::{marker::PhantomData, mem::ManuallyDrop};

use crate::{
    com::{self, WorkerId},
    error::{self, Result},
    signal::{self, Signal},
    TaskContext,
};

fn boxify<T: core::any::Any + Send>(b: T) -> Box<dyn core::any::Any + Send> {
    unsafe { Box::from_raw(Box::into_raw(Box::new(b)) as *mut (dyn core::any::Any + Send)) }
}

/// `[TaskBuilder]` is used to build new tasks from a given context.
pub struct TaskBuilder<'c, C, S> {
    ctx: &'c C,
    _mark: PhantomData<S>,
}

#[cfg(feature = "global")]
impl Default for TaskBuilder<'static, crate::GlobalContext, crate::global::Signal> {
    fn default() -> Self {
        Self::from_ctx(crate::GlobalContext::get())
    }
}

impl<'c, C: TaskContext, S: Signal + Default> TaskBuilder<'c, C, S> {
    /// Creates the `[TaskBuilder]` based on the context `C`.
    pub fn from_ctx(ctx: &'c C) -> Self {
        Self {
            ctx,
            _mark: PhantomData,
        }
    }

    /// Spawns a new task asynchronously.
    pub async fn spawn<T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> Handle<'static, T, S> {
        let handle = self
            .ctx
            .create_task(Box::new(move |id| Box::new(f(id))))
            .await;
        Handle::new(handle, S::default())
    }

    /// Spawns a new task.
    pub fn spawn_blocking<T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> BlockingHandle<'static, T, S> {
        signal::block_on_signal(S::default(), self.spawn(f)).into_blocking()
    }

    /// Spawns a new scoped task asynchronously.
    pub async fn spawn_scoped<'a: 'c, T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> Handle<'a, T, S> {
        // TODO: find a way to not use box when erasing the lifetime of this function?
        let f = unsafe {
            Box::from_raw(Box::into_raw(Box::new(f)) as *mut (dyn FnOnce(WorkerId) -> T + Send))
        };
        let handle = self
            .ctx
            .create_task(Box::new(move |id| boxify(f(id))))
            .await;
        Handle::new(handle, S::default())
    }

    /// Spawns a new scoped task.
    pub fn spawn_scoped_blocking<'a: 'c, T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> BlockingHandle<'a, T, S> {
        signal::block_on_signal(S::default(), self.spawn_scoped(f)).into_blocking()
    }
}

/// A handle to a task or scoped task.
/// This handle will automatically await the task on drop, which will discard the result
/// including potential errors.
///
/// If you want to get the value returned by the task, prefer using `[Self::wait]` or `[Self::wait_blocking]`
pub struct Handle<'a, T, S: Signal> {
    handle: ManuallyDrop<com::ComHandle>,
    signal: ManuallyDrop<S>,
    _mark: PhantomData<(&'a (), T)>,
}

impl<'a, T: 'static, S: Signal> Handle<'a, T, S> {
    fn new(handle: com::ComHandle, signal: S) -> Self {
        Self {
            handle: ManuallyDrop::new(handle),
            signal: ManuallyDrop::new(signal),
            _mark: PhantomData,
        }
    }

    /// If the task is finished executing, returns `true`, otherwise returns `false`.
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Awaits the current task asynchronously.
    pub async fn wait(self) -> Result<Box<T>> {
        let mut this = ManuallyDrop::new(self);
        unsafe {
            Ok(ManuallyDrop::take(&mut this.handle)
                .wait()
                .await?
                .downcast::<T>()
                .map_err(|_| error::display_error("failed to downcast"))?)
        }
    }

    /// Awaits the current task, blocks the current thread.
    fn wait_blocking(mut self) -> Result<Box<T>> {
        unsafe { signal::block_on_signal(ManuallyDrop::take(&mut self.signal), Self::wait(self)) }
    }

    /// Changes the handle into a `[BlockingHandle]`.
    pub fn into_blocking(self) -> BlockingHandle<'a, T, S> {
        BlockingHandle { inner: self }
    }
}

impl<'a, T, S: Signal> Drop for Handle<'a, T, S> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::take(&mut self.handle)
                .wait_blocking(ManuallyDrop::take(&mut self.signal))
                .expect("failed to await task");
        }
    }
}

/// A blocking handle to a task or scoped task.
/// This handle will automatically await the task on drop, which will discard the result
/// including potential errors.
///
/// If you want to get the value returned by the task, prefer using `[Self::wait_blocking]`
pub struct BlockingHandle<'a, T, S: Signal> {
    inner: Handle<'a, T, S>,
}

impl<'a, T: 'static, S: Signal> BlockingHandle<'a, T, S> {
    /// If the task is finished executing, returns `true`, otherwise returns `false`.
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }

    /// Awaits the current task, blocks the current thread.
    pub fn wait(self) -> Result<Box<T>> {
        self.inner.wait_blocking()
    }
}

#[cfg(feature = "global")]
mod global_helpers {
    use super::*;
    use std::future::Future;

    type Signal = crate::global::Signal;

    /// Spawns a task asynchronously.
    pub fn task<T: Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> impl Future<Output = Handle<'static, T, Signal>> {
        TaskBuilder::default().spawn(f)
    }

    /// Spawns a scoped task asynchronously.
    pub fn scoped<'a, T: Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> impl Future<Output = Handle<'a, T, Signal>> {
        TaskBuilder::default().spawn_scoped(f)
    }

    /// The blocking API for creating tasks without asynchronous code.
    pub mod blocking {
        use super::*;

        /// Spawns a task.
        pub fn task<T: Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'static,
        ) -> BlockingHandle<'static, T, Signal> {
            TaskBuilder::default().spawn_blocking(f)
        }

        /// Spawns a scoped task.
        pub fn scoped<'a, T: Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'a,
        ) -> BlockingHandle<'a, T, Signal> {
            TaskBuilder::default().spawn_scoped_blocking(f)
        }
    }
}

#[cfg(feature = "global")]
pub use global_helpers::*;
