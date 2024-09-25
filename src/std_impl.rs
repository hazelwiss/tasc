use core::mem::ManuallyDrop;
use crossbeam_channel as channel;
use std::{eprintln, sync::RwLock, vec::Vec};

use crate::{
    com::{self, WorkerId},
    TaskContext,
};

type WaitQueueSubscriber = channel::Sender<WorkerId>;

struct WaitQueue {
    receiver: channel::Receiver<WorkerId>,
    subscriber_creator: WaitQueueSubscriber,
}

impl WaitQueue {
    fn new() -> Self {
        let (sender, receiver) = channel::unbounded();
        Self {
            receiver,
            subscriber_creator: sender,
        }
    }

    fn creater_subscriber(&self) -> WaitQueueSubscriber {
        self.subscriber_creator.clone()
    }
}

enum WorkerMsg {
    Stop,
    Task {
        f: com::TaskFut,
        handle: com::JobHandle,
    },
}

struct Worker {
    incoming_msgs: channel::Receiver<WorkerMsg>,
    wait_queue: WaitQueueSubscriber,
    id: WorkerId,
    signal: alloc::sync::Arc<Signal>,
}

impl Worker {
    fn start(self) {
        loop {
            match std::panic::catch_unwind(|| loop {
                self.wait_queue.send(self.id).unwrap();
                match self.incoming_msgs.recv().unwrap() {
                    WorkerMsg::Stop => return,
                    WorkerMsg::Task { f, handle } => {
                        let res = crate::signal::block_on_signal_arc(
                            self.signal.clone(),
                            alloc::boxed::Box::into_pin(f),
                        );
                        handle.finish_job(res);
                    }
                }
            }) {
                Ok(_) => break,
                Err(e) => eprintln!("worked panicked: {e:?}"),
            }
        }
    }
}

struct WorkerCom {
    thread: ManuallyDrop<std::thread::JoinHandle<()>>,
    channel: channel::Sender<WorkerMsg>,
    #[allow(unused)]
    id: WorkerId,
}

struct ContextInner {
    handlers: Vec<WorkerCom>,
    wait_queue: WaitQueue,
}

impl ContextInner {
    fn find_ready_worker_id(&self) -> WorkerId {
        self.wait_queue.receiver.recv().unwrap()
    }

    fn send_msg(&self, worker_id: WorkerId, msg: WorkerMsg) {
        self.handlers[worker_id.id()].channel.send(msg).unwrap();
    }

    fn set_limit(&mut self, limit: usize) {
        if self.handlers.len() >= limit {
            return;
        }
        for i in self.handlers.len()..limit {
            let (sender, receiver) = channel::unbounded();
            let wait_queue = self.wait_queue.creater_subscriber();
            let id = WorkerId::new(i);
            let worker = Worker {
                incoming_msgs: receiver,
                id,
                wait_queue,
                signal: alloc::sync::Arc::new(Signal::new()),
            };
            let worker_thread = std::thread::Builder::new()
                .name(std::format!("tasc worker #{i}"))
                .spawn(move || worker.start())
                .unwrap_or_else(|e| panic!("failed to create thread for worker pool: {e}"));
            self.handlers.push(WorkerCom {
                thread: ManuallyDrop::new(worker_thread),
                channel: sender,
                id,
            });
        }
    }

    fn create_task(&self, f: com::TaskFut) -> com::ComHandle {
        let worker_id = self.find_ready_worker_id();
        let (job_handle, handle) = com::new_job_handles(worker_id);
        self.send_msg(
            worker_id,
            WorkerMsg::Task {
                f,
                handle: job_handle,
            },
        );
        handle
    }
}

/// The default context using crossbeam and the standard library.
///
/// This context facilitates creating new tasks and effectively dividing them among workers via a wait queue.
/// Creating new threads uses the Rust standard library [`std::thread::spawn`] with its default settings.
pub struct Context {
    inner: RwLock<ContextInner>,
}

impl Context {
    #[allow(missing_docs)]
    pub fn new(handlers: usize) -> Self {
        let this = Self {
            inner: RwLock::new(ContextInner {
                handlers: std::vec![],
                wait_queue: WaitQueue::new(),
            }),
        };
        this.set_workers(handlers);
        this
    }
}

impl crate::TaskContext for Context {
    fn set_workers(&self, max: usize) {
        self.inner.write().unwrap().set_limit(max)
    }

    fn workers(&self) -> usize {
        self.inner.read().unwrap().handlers.len()
    }

    fn create_task(&self, f: com::TaskFut) -> com::ComHandle {
        self.inner.read().unwrap().create_task(f)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        let mut write = self.inner.write().unwrap();
        for handle in &write.handlers {
            handle.channel.send(WorkerMsg::Stop).unwrap();
        }
        for handle in &mut write.handlers {
            unsafe {
                ManuallyDrop::take(&mut handle.thread)
                    .join()
                    .expect("failed to join thread");
            };
        }
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
