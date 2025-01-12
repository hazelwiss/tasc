use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crossbeam_channel as channel;
use std::{eprintln, sync::Arc};

use crate::{com, TaskContext};

struct Task {
    f: com::TaskFut,
    handle: com::JobHandle,
}

struct Worker {
    stack_size: Arc<AtomicUsize>,
    task_pool: channel::Receiver<Task>,
    keep_running: Arc<AtomicBool>,
    signal: alloc::sync::Arc<Signal>,
}

impl Worker {
    fn start(self) {
        loop {
            match std::panic::catch_unwind(|| loop {
                let stack_size = self.stack_size.load(Ordering::Relaxed);
                let stop = stacker::grow(stack_size, || loop {
                    if stack_size != self.stack_size.load(Ordering::Relaxed) {
                        return false;
                    }
                    if !self.keep_running.load(Ordering::Acquire) {
                        return true;
                    }
                    let Task { f, handle } = match self.task_pool.recv() {
                        Ok(ok) => ok,
                        // The channel was disconnected.
                        Err(_) => return true,
                    };
                    let res = crate::signal::block_on_signal_arc(
                        self.signal.clone(),
                        alloc::boxed::Box::into_pin(f),
                    );
                    handle.finish_job(res);
                });
                if stop {
                    return;
                }
            }) {
                Ok(_) => break,
                Err(e) => eprintln!("worked panicked: {e:?}"),
            }
        }
    }
}

struct ContextInner {
    keep_running: Arc<AtomicBool>,
    stack_size: Arc<AtomicUsize>,
    task_pool: crossbeam_channel::Sender<Task>,
    task_pool_recv: crossbeam_channel::Receiver<Task>,
    handlers: AtomicUsize,
}

impl ContextInner {
    fn set_limit(&self, limit: usize) {
        let current = self.handlers.load(Ordering::Acquire);
        if current >= limit {
            return;
        }
        self.handlers.store(limit, Ordering::Release);
        for i in current..limit {
            let worker = Worker {
                keep_running: self.keep_running.clone(),
                stack_size: self.stack_size.clone(),
                task_pool: self.task_pool_recv.clone(),
                signal: alloc::sync::Arc::new(Signal::new()),
            };
            std::thread::Builder::new()
                .name(std::format!("tasc worker #{i}"))
                .spawn(move || worker.start())
                .unwrap_or_else(|e| panic!("failed to create thread for worker pool: {e}"));
        }
    }

    fn create_task(&self, f: com::TaskFut) -> com::ComHandle {
        let (job_handle, handle) = com::new_job_handles();
        self.task_pool
            .send(Task {
                f,
                handle: job_handle,
            })
            .expect("failed to register task to task pool");
        handle
    }
}

/// The default context using crossbeam and the standard library.
///
/// This context facilitates creating new tasks and effectively dividing them among workers via a wait queue.
/// Creating new threads uses the Rust standard library [`std::thread::spawn`] with its default settings.
pub struct Context {
    inner: ContextInner,
}

impl Context {
    #[allow(missing_docs)]
    pub fn new(handlers: usize) -> Self {
        let (task_pool, task_pool_recv) = crossbeam_channel::unbounded();
        let this = Self {
            inner: ContextInner {
                // 1 MiB is the default stack size per worker.
                stack_size: Arc::new(AtomicUsize::new(1024 * 1024)),
                keep_running: Arc::new(AtomicBool::new(true)),
                task_pool,
                task_pool_recv,
                handlers: AtomicUsize::new(0),
            },
        };
        this.set_workers(handlers);
        this
    }

    #[allow(missing_docs)]
    pub fn task(&self) -> super::TaskBuilder<'_, Self, Signal> {
        super::TaskBuilder::from_ctx(self)
    }
}

impl crate::TaskContext for Context {
    fn set_workers(&self, max: usize) {
        self.inner.set_limit(max)
    }

    fn set_worker_stack_size(&self, stack_size: usize) {
        self.inner.stack_size.store(stack_size, Ordering::SeqCst);
    }

    fn workers(&self) -> usize {
        self.inner.handlers.load(Ordering::Acquire)
    }

    fn create_task(&self, f: com::TaskFut) -> com::ComHandle {
        self.inner.create_task(f)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        self.inner.keep_running.store(false, Ordering::Release);
    }
}

mod signal {
    use alloc::sync::Arc;
    use core::ops::Deref;
    use std::sync::{Condvar, Mutex};

    pub enum SignalState {
        None,
        Waiting,
        Notified,
    }

    /// The signal used by [`StdContext`], which makes use of [`CondVar`] from the standard library
    /// to efficiently await futures without wasting energy in a spinloop.
    pub struct Signal {
        state: Mutex<SignalState>,
        condvar: Condvar,
    }

    impl Default for Signal {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Signal {
        #[allow(missing_docs)]
        pub fn new() -> Self {
            Self {
                state: Mutex::new(SignalState::None),
                condvar: Condvar::new(),
            }
        }

        fn wait(&self) {
            let mut state = self.state.lock().unwrap();
            match *state {
                SignalState::None => {
                    *state = SignalState::Waiting;
                    while matches!(*state, SignalState::Waiting) {
                        state = self.condvar.wait(state).unwrap();
                    }
                }
                SignalState::Waiting => unreachable!(),
                SignalState::Notified => *state = SignalState::None,
            }
        }

        fn notify(&self) {
            let mut state = self.state.lock().unwrap();
            match *state {
                SignalState::None => *state = SignalState::Notified,
                SignalState::Waiting => {
                    *state = SignalState::None;
                    self.condvar.notify_one();
                }
                SignalState::Notified => {}
            }
        }
    }

    impl Signal {
        fn wake(self: Arc<Self>) {
            self.notify()
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.notify()
        }

        fn clone_waker(self: Arc<Self>) -> core::task::RawWaker {
            Self::raw_waker(Arc::clone(&self))
        }

        fn raw_waker(self: Arc<Self>) -> core::task::RawWaker {
            core::task::RawWaker::new(
                Arc::into_raw(self) as *const (),
                &core::task::RawWakerVTable::new(
                    |ptr| unsafe {
                        Self::clone_waker(
                            core::mem::ManuallyDrop::new(Arc::from_raw(ptr.cast::<Signal>()))
                                .deref()
                                .clone(),
                        )
                    },
                    |ptr| unsafe { (Arc::from_raw(ptr.cast::<Signal>())).wake() },
                    |ptr| unsafe {
                        core::mem::ManuallyDrop::new(Arc::from_raw(ptr.cast::<Signal>()))
                            .wake_by_ref()
                    },
                    |ptr| unsafe {
                        (Arc::from_raw(ptr.cast::<Signal>()));
                    },
                ),
            )
        }
    }

    impl crate::Signal for Signal {
        fn wait(&self) {
            Self::wait(self)
        }

        fn waker(self: Arc<Self>) -> core::task::Waker {
            unsafe { core::task::Waker::from_raw(self.raw_waker()) }
        }
    }
}

pub use signal::Signal;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn single_task() {
        let ctx = Context::new(1);

        let ticked_value = AtomicUsize::new(0);

        let t0 = ctx
            .task()
            .spawn_scoped_sync(|| ticked_value.fetch_add(1, Ordering::SeqCst));

        let t1 = ctx
            .task()
            .spawn_scoped_sync(|| ticked_value.fetch_add(1, Ordering::SeqCst));

        let t2 = ctx
            .task()
            .spawn_scoped_sync(|| ticked_value.fetch_add(1, Ordering::SeqCst));

        let t3 = ctx
            .task()
            .spawn_scoped_sync(|| ticked_value.fetch_add(1, Ordering::SeqCst));

        let t4 = ctx
            .task()
            .spawn_scoped_sync(|| ticked_value.fetch_add(1, Ordering::SeqCst));

        t0.wait().unwrap();
        t1.wait().unwrap();
        t2.wait().unwrap();
        t3.wait().unwrap();
        t4.wait().unwrap();

        assert_eq!(ticked_value.load(Ordering::SeqCst), 5);
    }
}
