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
        f: com::TaskFn,
        handle: com::JobHandle,
    },
}

struct Worker {
    incoming_msgs: channel::Receiver<WorkerMsg>,
    wait_queue: WaitQueueSubscriber,
    id: WorkerId,
}

impl Worker {
    fn start(self) {
        loop {
            match std::panic::catch_unwind(|| loop {
                self.wait_queue.send(self.id).unwrap();
                match self.incoming_msgs.recv().unwrap() {
                    WorkerMsg::Stop => return,
                    WorkerMsg::Task { f, handle } => {
                        let res = f(handle.worker_id());
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
    _handle: std::thread::JoinHandle<()>,
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
            };
            let worker_thread = std::thread::Builder::new()
                .name(std::format!("tasc worker #{i}"))
                .spawn(move || worker.start())
                .unwrap_or_else(|e| panic!("failed to create thread for worker pool: {e}"));
            self.handlers.push(WorkerCom {
                _handle: worker_thread,
                channel: sender,
                id,
            });
        }
    }

    fn create_task(&self, f: com::TaskFn) -> com::ComHandle {
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

pub struct Context {
    inner: RwLock<ContextInner>,
}

impl Context {
    pub async fn new(handlers: usize) -> Self {
        let this = Self {
            inner: RwLock::new(ContextInner {
                handlers: std::vec![],
                wait_queue: WaitQueue::new(),
            }),
        };
        this.set_limit(handlers).await;
        this
    }

    pub fn new_blocking(handlers: usize) -> Self {
        crate::signal::block_on_signal(Signal::new(), Self::new(handlers))
    }
}

impl crate::TaskContext for Context {
    async fn set_limit(&self, max: usize) {
        self.inner.write().unwrap().set_limit(max)
    }

    async fn create_task(&self, f: com::TaskFn) -> com::ComHandle {
        self.inner.read().unwrap().create_task(f)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        let write = self.inner.read().unwrap();
        for handle in &write.handlers {
            handle.channel.send(WorkerMsg::Stop).unwrap();
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
        pub fn new() -> Self {
            Self {
                state: Mutex::new(SignalState::None),
                condvar: Condvar::new(),
            }
        }

        pub fn wait(&self) {
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

        pub fn notify(&self) {
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

    impl crate::signal::Signal for Signal {
        fn wait(&self) {
            Self::wait(self)
        }

        fn waker(self: Arc<Self>) -> core::task::Waker {
            unsafe { core::task::Waker::from_raw(self.raw_waker()) }
        }
    }
}

pub use signal::Signal;
