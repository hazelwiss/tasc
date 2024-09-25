use alloc::boxed::Box;
use core::{marker::PhantomData, mem::ManuallyDrop};

use crate::{
    com::{self, WorkerId},
    error::{self, Result},
    signal::{self, Signal},
    TaskContext,
};

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
    ) -> com::ComHandle {
        self.ctx.create_task(Box::new(move |id| Box::new(f(id))))
    }

    /// Spawns a new task asynchronously.
    pub fn spawn<T: Unpin + Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> handles::AsyncHandle<T, S> {
        let handle = self.create_task(f);
        handles::AsyncHandle::new(handle, S::default())
    }

    /// Spawns a new task.
    pub fn spawn_sync<T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> handles::SyncHandle<T, S> {
        let handle = self.create_task(f);
        handles::SyncHandle::new(handle, S::default())
    }

    /// Spawns a new scoped task asynchronously.
    pub fn spawn_scoped<'a: 'c, T: Unpin + Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> handles::AsyncScopedHandle<'a, T, S>
    where
        S: 'static,
    {
        // TODO: find a way to not use box when erasing the lifetime of this function?
        let f = unsafe {
            Box::from_raw(Box::into_raw(Box::new(f)) as *mut (dyn FnOnce(WorkerId) -> T + Send))
        };
        let handle = self.create_task(f);
        handles::AsyncScopedHandle::new(handle, S::default())
    }

    /// Spawns a new scoped task.
    pub fn spawn_scoped_sync<'a: 'c, T: Send + 'static>(
        self,
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> handles::SyncScopedHandle<'a, T, S>
    where
        S: 'static,
    {
        // TODO: find a way to not use box when erasing the lifetime of this function?
        let f = unsafe {
            Box::from_raw(Box::into_raw(Box::new(f)) as *mut (dyn FnOnce(WorkerId) -> T + Send))
        };
        let handle = self.create_task(f);
        handles::SyncScopedHandle::new(handle, S::default())
    }
}

/// Generic task handle types.
pub mod handles {
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

        fn is_finished(&self) -> bool {
            self.inner.0.is_finished()
        }

        /// # Safety
        /// This function call invalidates `self` and it is no longer safe to use
        /// `self` afer this call returns.
        unsafe fn decompose(&mut self) -> (com::ComHandle, S) {
            ManuallyDrop::take(&mut self.inner)
        }

        fn poll_handle(handle: com::ComHandle, signal: S) -> Result<T> {
            signal::block_on_signal(signal, handle)?
                .downcast::<T>()
                .map(|val| *val)
                .map_err(|_| error::display_error("failed to downcast"))
        }
    }

    /// An asynchronous task handle. Can be awaited to get the results of a task.
    pub struct AsyncHandle<T, S> {
        inner: InnerHandle<T, S>,
    }

    impl<T: Send + Unpin + 'static, S: Signal> AsyncHandle<T, S> {
        pub(crate) fn new(handle: com::ComHandle, signal: S) -> Self {
            Self {
                inner: InnerHandle::new(handle, signal),
            }
        }

        /// See if the handle's task has been finished.
        pub fn is_finished(&self) -> bool {
            self.inner.is_finished()
        }

        #[doc(hidden)]
        pub fn into_raw_handle_and_signal(mut self) -> (com::ComHandle, impl Signal) {
            // SAFETY: this function moves 'self'
            unsafe { self.inner.decompose() }
        }

        #[doc(hidden)]
        pub fn into_sync(self) -> SyncHandle<T, S> {
            SyncHandle { inner: self.inner }
        }
    }

    impl<T: Send + Unpin + 'static, S: Signal> core::future::Future for AsyncHandle<T, S> {
        type Output = Result<T>;

        fn poll(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            if self.is_finished() {
                core::task::Poll::Ready({
                    // SAFETY: after a value has been polled by await, it is no longer
                    let (handle, signal) = unsafe { self.get_mut().inner.decompose() };
                    InnerHandle::poll_handle(handle, signal)
                })
            } else {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        }
    }

    /// A synchronous task handle. Can be awaited to get the results of a task.
    pub struct SyncHandle<T: Send, S: Signal> {
        inner: InnerHandle<T, S>,
    }

    impl<T: Send + 'static, S: Signal> SyncHandle<T, S> {
        pub(crate) fn new(handle: com::ComHandle, signal: S) -> Self {
            Self {
                inner: InnerHandle::new(handle, signal),
            }
        }

        /// See if the handle's task has been finished.
        pub fn is_finished(&self) -> bool {
            self.inner.is_finished()
        }

        #[doc(hidden)]
        pub fn into_raw_handle_and_signal(mut self) -> (com::ComHandle, impl Signal) {
            // SAFETY: this function moves 'self'
            unsafe { self.inner.decompose() }
        }

        /// Blocks the current thread awaiting the task to complete.
        pub fn wait(self) -> Result<T> {
            let (handle, signal) = self.into_raw_handle_and_signal();
            InnerHandle::poll_handle(handle, signal)
        }
    }

    /// A scoped asynchronous task that captures the lifetimes of borrowed values.
    /// This type will await the task to completion upon drop.
    pub struct AsyncScopedHandle<'a, T: Send + 'static, S: Signal> {
        handle: ManuallyDrop<InnerHandle<T, S>>,
        exhausted: bool,
        _mark: PhantomData<&'a ()>,
    }

    impl<'a, T: Send + 'static, S: Signal> AsyncScopedHandle<'a, T, S> {
        pub(crate) fn new(handle: com::ComHandle, signal: S) -> Self {
            Self {
                handle: ManuallyDrop::new(InnerHandle::new(handle, signal)),
                exhausted: false,
                _mark: PhantomData,
            }
        }

        /// See if the handle's task has been finished.
        pub fn is_finished(&self) -> bool {
            self.handle.is_finished()
        }

        #[doc(hidden)]
        pub fn into_raw_handle_and_signal(mut self) -> (com::ComHandle, impl Signal) {
            // SAFETY: this function moves 'self'
            unsafe { self.handle.decompose() }
        }

        #[doc(hidden)]
        pub fn into_sync(self) -> SyncScopedHandle<'a, T, S> {
            let mut this = ManuallyDrop::new(self);
            SyncScopedHandle::<'a, T, S> {
                handle: ManuallyDrop::new(unsafe { ManuallyDrop::take(&mut this.handle) }),
                _mark: PhantomData,
            }
        }
    }

    impl<'a, T: Unpin + Send + 'static, S: Signal> core::future::Future
        for AsyncScopedHandle<'a, T, S>
    {
        type Output = Result<T>;

        fn poll(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            if !self.exhausted && self.is_finished() {
                let this = self.get_mut();
                this.exhausted = true;
                // SAFETY: Because of our check with exhausted above, this is guaranteed to never happen.
                let (handle, signal) = unsafe { ManuallyDrop::take(&mut this.handle).decompose() };
                core::task::Poll::Ready(InnerHandle::poll_handle(handle, signal))
            } else {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        }
    }

    impl<'a, T: Send + 'static, S: Signal> Drop for AsyncScopedHandle<'a, T, S> {
        fn drop(&mut self) {
            unsafe {
                let (com, signal) = ManuallyDrop::take(&mut self.handle).decompose();
                let _ = com.wait_blocking(signal);
            }
        }
    }

    /// A scoped synchronous task that captures the lifetimes of borrowed values.
    /// This type will await the task to completion upon drop.
    pub struct SyncScopedHandle<'a, T: Send + 'static, S: Signal> {
        handle: ManuallyDrop<InnerHandle<T, S>>,
        _mark: PhantomData<&'a ()>,
    }

    impl<'a, T: Send + 'static, S: Signal> SyncScopedHandle<'a, T, S> {
        pub(crate) fn new(handle: com::ComHandle, signal: S) -> Self {
            Self {
                handle: ManuallyDrop::new(InnerHandle::new(handle, signal)),
                _mark: PhantomData,
            }
        }

        /// See if the handle's task has been finished.
        pub fn is_finished(&self) -> bool {
            self.handle.is_finished()
        }

        #[doc(hidden)]
        pub fn into_raw_handle_and_signal(mut self) -> (com::ComHandle, impl Signal) {
            // SAFETY: this function moves 'self'
            unsafe { self.handle.decompose() }
        }

        /// Blocks the current thread awaiting the task to complete.
        pub fn wait(self) -> Result<T> {
            let mut this = ManuallyDrop::new(self);
            unsafe {
                let (com, signal) = this.handle.decompose();
                InnerHandle::poll_handle(com, signal)
            }
        }
    }

    impl<'a, T: Send + 'static, S: Signal> Drop for SyncScopedHandle<'a, T, S> {
        fn drop(&mut self) {
            unsafe {
                let (com, signal) = ManuallyDrop::take(&mut self.handle).decompose();
                let _ = com.wait_blocking(signal);
            }
        }
    }
}

#[cfg(feature = "global")]
mod global_helpers {
    use super::*;

    /// See [`handles::AsyncHandle`] for more details.
    pub type AsyncHandle<T> = handles::AsyncHandle<T, crate::StdSignal>;
    /// See [`handles::AsyncScopedHandle`] for more details.
    pub type AsyncScopedHandle<'a, T> = handles::AsyncScopedHandle<'a, T, crate::StdSignal>;

    /// Spawns an asynchronous task.
    pub fn task<T: Unpin + Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'static,
    ) -> AsyncHandle<T> {
        TaskBuilder::default().spawn(f)
    }

    /// Spawns a scoped asynchronous task.
    pub fn scoped<'a, T: Unpin + Send + 'static>(
        f: impl FnOnce(WorkerId) -> T + Send + 'a,
    ) -> AsyncScopedHandle<'a, T> {
        TaskBuilder::default().spawn_scoped(f)
    }

    /// The sync API for creating tasks without asynchronous code.
    pub mod sync {
        use super::*;

        /// See [`handles::SyncHandle`] for more details.
        pub type SyncHandle<T> = handles::SyncHandle<T, crate::StdSignal>;
        /// See [`handles::SyncScopedHandle`] for more details.
        pub type SyncScopedHandle<'a, T> = handles::SyncScopedHandle<'a, T, crate::StdSignal>;

        /// Spawns a synchronous task.
        pub fn task<T: Unpin + Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'static,
        ) -> SyncHandle<T> {
            TaskBuilder::default().spawn_sync(f)
        }

        /// Spawns a scoped synchronous task.
        pub fn scoped<'a, T: Unpin + Send + 'static>(
            f: impl FnOnce(WorkerId) -> T + Send + 'a,
        ) -> SyncScopedHandle<'a, T> {
            TaskBuilder::default().spawn_scoped_sync(f)
        }
    }
}

#[cfg(feature = "global")]
pub use global_helpers::*;
