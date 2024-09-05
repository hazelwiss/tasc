use alloc::boxed::Box;
use core::{marker::PhantomData, mem::ManuallyDrop};

use crate::{
    com::{self, WorkerId},
    error::{self, Result},
    signal::{self, Signal},
    TaskContext,
};

/// A generic Task Handle. This can be used to retrieve the raw, underlying handle and signal,
/// but also to check if the given task has finished.
pub trait TaskHandle<T>: Send {
    #[allow(missing_docs)]
    fn is_finished(&self) -> bool;

    #[allow(missing_docs)]
    fn into_raw_handle_and_signal(self) -> (com::ComHandle, impl Signal);
}

/// A handle capable of asynchronously running a task. This is useful for when you're working
/// inside of an asynchronous context.
pub trait AsyncTaskHandle<T>: TaskHandle<T> + core::future::Future<Output = Result<T>> {
    #[allow(missing_docs)]
    fn into_blocking(self) -> impl BlockingTaskHandle<T>;
}

/// A handle capable of running a task. When the task is joined, it will block the current thread.
pub trait BlockingTaskHandle<T>: TaskHandle<T> {
    #[allow(missing_docs)]
    fn wait(self) -> Result<T>;
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

    fn create_task<T: Send + 'static>(
        &self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> impl core::future::Future<Output = com::ComHandle> + 'c {
        self.ctx.create_task(Box::new(move |id| Box::new(f(id))))
    }

    /// Spawns a new task asynchronously.
    pub async fn spawn<T: Unpin + Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> impl AsyncTaskHandle<T> {
        let handle = self.create_task(f).await;
        imp::Handle::new(handle, S::default())
    }

    /// Spawns a new task.
    pub fn spawn_blocking<T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> impl BlockingTaskHandle<T> {
        signal::block_on_signal(S::default(), async move {
            let handle = self.create_task(f).await;
            imp::BlockingHandle::new(handle, S::default())
        })
    }

    /// Spawns a new scoped task asynchronously.
    pub async fn spawn_scoped<'a: 'c, T: Unpin + Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> impl AsyncTaskHandle<T> + 'a
    where
        S: 'static,
    {
        // TODO: find a way to not use box when erasing the lifetime of this function?
        let f = unsafe {
            Box::from_raw(Box::into_raw(Box::new(f)) as *mut (dyn FnOnce(WorkerId) -> T + Send))
        };
        let handle = self.create_task(f).await;
        imp::Scoped::new(imp::Handle::new(handle, S::default()))
    }

    /// Spawns a new scoped task.
    pub fn spawn_scoped_blocking<'a: 'c, T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> impl BlockingTaskHandle<T> + 'a
    where
        S: 'static,
    {
        // TODO: find a way to not use box when erasing the lifetime of this function?
        let f = unsafe {
            Box::from_raw(Box::into_raw(Box::new(f)) as *mut (dyn FnOnce(WorkerId) -> T + Send))
        };
        signal::block_on_signal(S::default(), async move {
            let handle = self.create_task(f).await;
            imp::Scoped::new(imp::BlockingHandle::new(handle, S::default()))
        })
    }
}

mod imp {
    use super::*;

    struct InnerHandle<T, S> {
        inner: ManuallyDrop<(com::ComHandle, S)>,
        _mark: PhantomData<T>,
    }

    impl<T: Send + 'static, S: Signal> InnerHandle<T, S> {
        fn new(handle: com::ComHandle, signal: S) -> Self {
            Self {
                inner: ManuallyDrop::new((handle, signal)),
                _mark: PhantomData,
            }
        }

        fn decompose(&mut self) -> (com::ComHandle, S) {
            unsafe { ManuallyDrop::take(&mut self.inner) }
        }

        fn poll_handle(handle: com::ComHandle, signal: S) -> Result<T> {
            signal::block_on_signal(signal, handle)?
                .downcast::<T>()
                .map(|val| *val)
                .map_err(|_| error::display_error("failed to downcast"))
        }
    }

    impl<T: Send + 'static, S: Signal> TaskHandle<T> for InnerHandle<T, S> {
        fn is_finished(&self) -> bool {
            self.inner.0.is_finished()
        }

        fn into_raw_handle_and_signal(mut self) -> (com::ComHandle, impl Signal) {
            unsafe { ManuallyDrop::take(&mut self.inner) }
        }
    }

    pub struct Handle<T, S> {
        inner: InnerHandle<T, S>,
    }

    impl<T: Send + Unpin + 'static, S: Signal> Handle<T, S> {
        pub fn new(handle: com::ComHandle, signal: S) -> Self {
            Self {
                inner: InnerHandle::new(handle, signal),
            }
        }
    }

    impl<T: Send + Unpin + 'static, S: Signal> core::future::Future for Handle<T, S> {
        type Output = Result<T>;

        fn poll(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            if self.is_finished() {
                core::task::Poll::Ready({
                    let (handle, signal) = self.get_mut().inner.decompose();
                    InnerHandle::poll_handle(handle, signal)
                })
            } else {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        }
    }

    impl<T: Send + Unpin + 'static, S: Signal> TaskHandle<T> for Handle<T, S> {
        fn is_finished(&self) -> bool {
            self.inner.is_finished()
        }

        fn into_raw_handle_and_signal(self) -> (com::ComHandle, impl Signal) {
            self.inner.into_raw_handle_and_signal()
        }
    }

    impl<T: Send + Unpin + 'static, S: Signal> AsyncTaskHandle<T> for Handle<T, S> {
        fn into_blocking(self) -> impl BlockingTaskHandle<T> {
            BlockingHandle { inner: self.inner }
        }
    }

    pub struct BlockingHandle<T: Send, S: Signal> {
        inner: InnerHandle<T, S>,
    }

    impl<T: Send + 'static, S: Signal> BlockingHandle<T, S> {
        pub fn new(handle: com::ComHandle, signal: S) -> Self {
            Self {
                inner: InnerHandle::new(handle, signal),
            }
        }
    }

    impl<T: Send + 'static, S: Signal> TaskHandle<T> for BlockingHandle<T, S> {
        fn is_finished(&self) -> bool {
            self.inner.is_finished()
        }

        fn into_raw_handle_and_signal(mut self) -> (com::ComHandle, impl Signal) {
            InnerHandle::decompose(&mut self.inner)
        }
    }

    impl<T: Send + 'static, S: Signal> BlockingTaskHandle<T> for BlockingHandle<T, S> {
        fn wait(mut self) -> Result<T> {
            let (handle, signal) = self.inner.decompose();
            InnerHandle::poll_handle(handle, signal)
        }
    }

    pub struct Scoped<'a, T, H: TaskHandle<T>> {
        handle: ManuallyDrop<H>,
        should_drop: bool,
        _mark: PhantomData<(&'a (), T)>,
    }

    impl<'a, T, H: TaskHandle<T>> Scoped<'a, T, H> {
        pub fn new(handle: H) -> Self {
            Self {
                handle: ManuallyDrop::new(handle),
                should_drop: true,
                _mark: PhantomData,
            }
        }
    }

    impl<'a, T: Send, H: TaskHandle<T>> TaskHandle<T> for Scoped<'a, T, H> {
        fn is_finished(&self) -> bool {
            self.handle.is_finished()
        }

        fn into_raw_handle_and_signal(mut self) -> (com::ComHandle, impl Signal) {
            unsafe { ManuallyDrop::take(&mut self.handle).into_raw_handle_and_signal() }
        }
    }

    impl<'a, T: Unpin + Send + 'static, H: AsyncTaskHandle<T> + Unpin> AsyncTaskHandle<T>
        for Scoped<'a, T, H>
    {
        fn into_blocking(self) -> impl BlockingTaskHandle<T> {
            let mut this = ManuallyDrop::new(self);
            Scoped::<'a, T, _> {
                handle: ManuallyDrop::new(unsafe {
                    ManuallyDrop::take(&mut this.handle).into_blocking()
                }),
                should_drop: true,
                _mark: PhantomData,
            }
        }
    }

    impl<'a, T: Unpin + Send + 'static, H: AsyncTaskHandle<T> + Unpin> core::future::Future
        for Scoped<'a, T, H>
    {
        type Output = Result<T>;

        fn poll(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            if self.is_finished() {
                let this = self.get_mut();
                this.should_drop = false;
                let (handle, signal) =
                    unsafe { ManuallyDrop::take(&mut this.handle).into_raw_handle_and_signal() };
                core::task::Poll::Ready(InnerHandle::poll_handle(handle, signal))
            } else {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        }
    }

    impl<'a, T: Send, H: BlockingTaskHandle<T>> BlockingTaskHandle<T> for Scoped<'a, T, H> {
        fn wait(self) -> Result<T> {
            let mut this = ManuallyDrop::new(self);
            unsafe { ManuallyDrop::take(&mut this.handle).wait() }
        }
    }

    impl<'a, T, H: TaskHandle<T>> Drop for Scoped<'a, T, H> {
        fn drop(&mut self) {
            if self.should_drop {
                let (handle, signal) =
                    unsafe { ManuallyDrop::take(&mut self.handle).into_raw_handle_and_signal() };
                let _ = handle.wait_blocking(signal);
            }
        }
    }
}

#[cfg(feature = "global")]
mod global_helpers {
    use super::*;
    use std::future::Future;

    /// Spawns a task asynchronously.
    pub fn task<T: Unpin + Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> impl Future<Output = impl AsyncTaskHandle<T>> {
        TaskBuilder::default().spawn(f)
    }

    /// Spawns a scoped task asynchronously.
    pub fn scoped<'a, T: Unpin + Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> impl Future<Output = impl AsyncTaskHandle<T> + 'a> {
        TaskBuilder::default().spawn_scoped(f)
    }

    /// The blocking API for creating tasks without asynchronous code.
    pub mod blocking {
        use super::*;

        /// Spawns a task.
        pub fn task<T: Unpin + Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'static,
        ) -> impl BlockingTaskHandle<T> {
            TaskBuilder::default().spawn_blocking(f)
        }

        /// Spawns a scoped task.
        pub fn scoped<'a, T: Unpin + Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'a,
        ) -> impl BlockingTaskHandle<T> + 'a {
            TaskBuilder::default().spawn_scoped_blocking(f)
        }
    }
}

#[cfg(feature = "global")]
pub use global_helpers::*;
