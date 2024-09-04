//! Contains shared types for communication between a task and its handle in a way that is independent from the context.

use core::{future::Future, mem::ManuallyDrop};

use alloc::{boxed::Box, sync::Arc};
use spin::Mutex;

use crate::{error, signal, Signal};

/// Uniquely identifies a Worker Thread.
#[derive(Clone, Copy, Debug)]
pub struct WorkerId(usize);

impl WorkerId {
    /// Creates a Worker ID. Must be unique for the context.
    pub fn new(id: usize) -> Self {
        Self(id)
    }

    #[allow(missing_docs)]
    pub fn id(self) -> usize {
        self.0
    }
}

impl core::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

#[allow(missing_docs)]
pub type TaskFn = Box<dyn FnOnce(WorkerId) -> Box<dyn core::any::Any + Send + 'static> + Send>;

struct ConnectionState {
    result: Mutex<Option<Box<dyn core::any::Any + Send>>>,
}

/// The handle for an ongoing job. This is used by the context to provide a result to a given task.
pub struct JobHandle {
    id: WorkerId,
    connection: Arc<ConnectionState>,
}

impl JobHandle {
    /// Called by the context when a task is finished, and the result can by sent to the Handle
    pub fn finish_job(&self, result: Box<dyn core::any::Any + Send>) {
        let connection = &self.connection;
        *connection.result.lock() = Some(result);
    }

    #[allow(missing_docs)]
    pub fn worker_id(&self) -> WorkerId {
        self.id
    }
}

/// Used by the context to maintain communications wit the `[JobHandle]` and see the status of the ongoing task
/// as well as retrieve the result once the task is finished.
pub struct ComHandle {
    #[allow(unused)]
    id: WorkerId,
    connection: Arc<ConnectionState>,
}

impl ComHandle {
    /// Returns `true` if the task is finished, otherwise `false`
    pub fn is_finished(&self) -> bool {
        self.connection
            .result
            .try_lock()
            .map(|lock| lock.is_some())
            .unwrap_or(false)
    }

    /// Awaits the current task until it is finished asynchronously.
    pub fn wait(self) -> impl Future<Output = error::Result<Box<dyn core::any::Any + Send>>> {
        let this = ManuallyDrop::new(self);
        core::future::poll_fn(move |cx| {
            if this.is_finished() {
                core::task::Poll::Ready(Ok(this.connection.result.lock().take().unwrap()))
            } else {
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        })
    }

    /// Awaits the current task until it is finished, blocks the current thread.
    pub fn wait_blocking(
        self,
        signal: impl Signal,
    ) -> error::Result<Box<dyn core::any::Any + Send>> {
        signal::block_on_signal(signal, self.wait())
    }
}

/// A utility function for contexts to create handles.
/// Every worker must have a unique ID.
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
