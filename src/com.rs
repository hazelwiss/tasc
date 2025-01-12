//! Contains shared types for communication between a task and its handle in a way that is independent from the context.

use alloc::{boxed::Box, sync::Arc};
use core::future::Future;
use spin::Mutex;

use crate::{error, signal, Signal};

#[allow(missing_docs)]
pub type TaskFut = Box<dyn Future<Output = Box<dyn core::any::Any + Send + 'static>> + Send>;

struct ConnectionState {
    result: Mutex<Option<Box<dyn core::any::Any + Send>>>,
}

/// The handle for an ongoing job. This is used by the context to provide a result to a given task.
pub struct JobHandle {
    connection: Arc<ConnectionState>,
}

impl JobHandle {
    /// Called by the context when a task is finished, and the result can by sent to [`ComHandle`].
    pub fn finish_job(&self, result: Box<dyn core::any::Any + Send>) {
        let connection = &self.connection;
        *connection.result.lock() = Some(result);
    }
}

/// Used to maintain communications with the [`JobHandle`] and see the status of the ongoing task.
/// Once the result is available, we retrive it from this type. This type is a future, and can be awaited.
pub struct ComHandle {
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

    /// Awaits the current task until it is finished, blocks the current thread.
    pub fn wait_blocking(
        self,
        signal: impl Signal,
    ) -> error::Result<Box<dyn core::any::Any + Send>> {
        signal::block_on_signal(signal, self)
    }
}

impl core::future::Future for ComHandle {
    type Output = error::Result<Box<dyn core::any::Any + Send>>;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if self.is_finished() {
            core::task::Poll::Ready(Ok(self.connection.result.lock().take().unwrap()))
        } else {
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    }
}

/// A utility function for contexts to create handles.
/// Every worker must have a unique ID.
pub fn new_job_handles() -> (JobHandle, ComHandle) {
    let state = Arc::new(ConnectionState {
        result: Mutex::new(None),
    });

    let job_handle = JobHandle {
        connection: state.clone(),
    };
    let handle = ComHandle {
        connection: state.clone(),
    };
    (job_handle, handle)
}
