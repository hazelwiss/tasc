use alloc::boxed::Box;
use core::{future::Future, marker::PhantomData, mem::ManuallyDrop};

use crate::{
    com::{self, CreateOpts, WorkerId},
    error::{self, Result},
    waker, GlobalContext, TaskContext,
};

fn boxify<T: std::any::Any + Send>(b: T) -> Box<dyn std::any::Any + Send> {
    unsafe { Box::from_raw(Box::into_raw(Box::new(b)) as *mut (dyn core::any::Any + Send)) }
}

pub struct ScopedSpawner<'c, C> {
    ctx: &'c C,
    opts: CreateOpts,
}

impl<'c, C: TaskContext> ScopedSpawner<'c, C> {
    pub async fn spawn<'a: 'c, T: Send + 'static>(
        &self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> ScopedHandle<'a, T> {
        // TODO: find a way to not use box when erasing the lifetime of this function?
        let f = unsafe {
            Box::from_raw(Box::into_raw(Box::new(f)) as *mut (dyn FnOnce(WorkerId) -> T + Send))
        };
        let handle = self
            .ctx
            .create_task(Box::new(move |id| boxify(f(id))))
            .await;
        ScopedHandle {
            handle: ManuallyDrop::new(handle),
            _mark: PhantomData,
        }
    }

    pub fn spawn_blocking<'a: 'c, T: Send + 'static>(
        &self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> ScopedHandle<'a, T> {
        waker::block_on(self.spawn(f))
    }

    async fn spawn_once<'a: 'c, T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> ScopedHandle<'a, T> {
        let f = unsafe {
            Box::from_raw(Box::into_raw(Box::new(f)) as *mut (dyn FnOnce(WorkerId) -> T + Send))
        };
        let handle = self
            .ctx
            .create_task(Box::new(move |id| boxify(f(id))))
            .await;
        ScopedHandle {
            handle: ManuallyDrop::new(handle),
            _mark: PhantomData,
        }
    }
}

pub struct TaskBuilder<'c, C> {
    ctx: &'c C,
    opts: CreateOpts,
}

#[cfg(feature = "global")]
impl Default for TaskBuilder<'static, crate::GlobalContext> {
    fn default() -> Self {
        Self::with_context(GlobalContext::get())
    }
}

impl<'c, C: TaskContext> TaskBuilder<'c, C> {
    pub fn with_context(ctx: &'c C) -> Self {
        Self {
            ctx,
            opts: CreateOpts::default(),
        }
    }

    pub async fn spawn<T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> Handle<T> {
        let handle = self
            .ctx
            .create_task(Box::new(move |id| Box::new(f(id))))
            .await;
        Handle {
            handle: ManuallyDrop::new(handle),
            _mark: PhantomData,
        }
    }

    pub fn spawn_blocking<T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> Handle<T> {
        waker::block_on(self.spawn(f))
    }

    pub fn spawn_scoped<'a: 'c, T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> impl Future<Output = ScopedHandle<'a, T>> + 'c {
        ScopedSpawner {
            ctx: self.ctx,
            opts: self.opts,
        }
        .spawn_once(f)
    }

    pub fn spawn_scoped_blocking<'a: 'c, T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> ScopedHandle<'a, T> {
        waker::block_on(self.spawn_scoped(f))
    }

    pub fn scope(self, f: impl FnOnce(ScopedSpawner<'c, C>)) {
        f(ScopedSpawner {
            ctx: self.ctx,
            opts: self.opts,
        })
    }
}

pub trait TaskHandle<T> {
    fn is_ready(&self) -> bool;

    fn wait(self) -> impl Future<Output = Result<Box<T>>>;
}

pub trait BlockingTaskHandle<T>: TaskHandle<T> {
    fn wait_blocking(self) -> Result<Box<T>>;
}

pub struct Handle<T> {
    handle: ManuallyDrop<com::ComHandle>,
    _mark: PhantomData<T>,
}

impl<T: 'static> TaskHandle<T> for Handle<T> {
    fn is_ready(&self) -> bool {
        self.handle.is_ready()
    }

    async fn wait(self) -> Result<Box<T>> {
        let mut this = ManuallyDrop::new(self);
        unsafe {
            Ok(ManuallyDrop::take(&mut this.handle)
                .wait()
                .await?
                .downcast::<T>()
                .map_err(|_| error::display_error("failed to downcast"))?)
        }
    }
}

impl<T: 'static> BlockingTaskHandle<T> for Handle<T> {
    fn wait_blocking(self) -> Result<Box<T>> {
        waker::block_on(Self::wait(self))
    }
}

impl<T> Drop for Handle<T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::take(&mut self.handle)
                .wait_blocking()
                .expect("failed to await task");
        }
    }
}

pub struct ScopedHandle<'a, T> {
    handle: ManuallyDrop<com::ComHandle>,
    _mark: PhantomData<(&'a (), T)>,
}

impl<'a, T: 'static> TaskHandle<T> for ScopedHandle<'a, T> {
    fn is_ready(&self) -> bool {
        self.handle.is_ready()
    }

    async fn wait(self) -> Result<Box<T>> {
        let mut this = ManuallyDrop::new(self);
        unsafe {
            Ok(ManuallyDrop::take(&mut this.handle)
                .wait()
                .await?
                .downcast::<T>()
                .map_err(|_| error::display_error("failed to downcast"))?)
        }
    }
}

impl<'a, T: 'static> BlockingTaskHandle<T> for ScopedHandle<'a, T> {
    fn wait_blocking(self) -> Result<Box<T>> {
        waker::block_on(Self::wait(self))
    }
}

impl<'a, T> Drop for ScopedHandle<'a, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::take(&mut self.handle)
                .wait_blocking()
                .expect("failed to await task");
        }
    }
}

#[cfg(feature = "global")]
mod global_helpers {
    use super::*;

    pub fn task<T: Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> impl Future<Output = Handle<T>> {
        TaskBuilder::default().spawn(f)
    }

    pub fn scoped<'a, T: Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> impl Future<Output = ScopedHandle<'a, T>> {
        TaskBuilder::default().spawn_scoped(f)
    }

    pub fn scope(f: impl FnOnce(ScopedSpawner<'static, GlobalContext>)) {
        TaskBuilder::default().scope(f)
    }

    pub mod blocking {
        use super::*;

        pub fn task<T: Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'static,
        ) -> Handle<T> {
            waker::block_on(TaskBuilder::default().spawn(f))
        }

        pub fn scoped<'a, T: Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'a,
        ) -> ScopedHandle<'a, T> {
            waker::block_on(TaskBuilder::default().spawn_scoped(f))
        }
    }
}

#[cfg(feature = "global")]
pub use global_helpers::*;
