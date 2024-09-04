use core::{future::Future, mem::ManuallyDrop};

use alloc::{boxed::Box, sync::Arc};
use spin::Mutex;

use crate::{error, signal, Signal};

#[derive(Clone, Copy, Debug)]
pub struct WorkerId(usize);

impl WorkerId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }

    pub fn id(self) -> usize {
        self.0
    }
}

pub type TaskFn = Box<dyn FnOnce(WorkerId) -> Box<dyn core::any::Any + Send + 'static> + Send>;

struct ConnectionState {
    result: Mutex<Option<Box<dyn core::any::Any + Send>>>,
}

pub struct JobHandle {
    id: WorkerId,
    connection: Arc<ConnectionState>,
}

impl JobHandle {
    pub fn finish_job(&self, result: Box<dyn core::any::Any + Send>) {
        let connection = &self.connection;
        *connection.result.lock() = Some(result);
    }

    pub fn worker_id(&self) -> WorkerId {
        self.id
    }
}

pub struct ComHandle {
    #[allow(unused)]
    id: WorkerId,
    connection: Arc<ConnectionState>,
}

impl ComHandle {
    pub fn is_ready(&self) -> bool {
        self.connection
            .result
            .try_lock()
            .map(|lock| lock.is_some())
            .unwrap_or(false)
    }

    pub fn wait(self) -> impl Future<Output = error::Result<Box<dyn core::any::Any + Send>>> {
        let this = ManuallyDrop::new(self);
        core::future::poll_fn(move |cx| {
            if this.is_ready() {
                core::task::Poll::Ready(Ok(this.connection.result.lock().take().unwrap()))
            } else {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        })
    }

    pub fn wait_blocking(
        self,
        signal: impl Signal,
    ) -> error::Result<Box<dyn core::any::Any + Send>> {
        signal::block_on_signal(signal, self.wait())
    }
}

pub fn new_job_handles(id: WorkerId) -> (JobHandle, ComHandle) {
    let state = Arc::new(ConnectionState {
        result: Mutex::new(None),
    });

    let job_handle = JobHandle {
        id,
        connection: state.clone(),
    };
    let handle = ComHandle {
        id,
        connection: state.clone(),
    };
    (job_handle, handle)
}
