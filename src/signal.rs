use alloc::sync::Arc;
use core::{future::Future, task::Context};

/// Provides an abstraction on a signal that is used for the synchronous API.
/// The signal is also used when a type is dropped, because asynchronous drops
/// are unstable.
pub trait Signal: Send + Unpin {
    /// Awaits the signal to trigger.
    fn wait(&self);

    /// Crates a waker from `[Self]`.
    fn waker(self: Arc<Self>) -> core::task::Waker;
}

/// Blocks asynchronous code based on a signal.
pub fn block_on_signal<F: Future>(signal: impl Signal, future: F) -> F::Output {
    let signal = Arc::new(signal);
    block_on_signal_arc(signal, future)
}

pub fn block_on_signal_arc<F: Future>(signal: Arc<impl Signal>, mut future: F) -> F::Output {
    let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };
    let waker = Signal::waker(signal.clone());

    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(value) => break value,
            core::task::Poll::Pending => Signal::wait(&*signal),
        }
    }
}
