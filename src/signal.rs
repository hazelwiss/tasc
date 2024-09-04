use alloc::sync::Arc;
use core::{future::Future, task::Context};

pub trait Signal {
    fn wait(&self);

    fn waker(self: Arc<Self>) -> core::task::Waker;
}

pub fn block_on_signal<F: Future>(signal: impl Signal, mut future: F) -> F::Output {
    let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };

    let signal = Arc::new(signal);
    let waker = Signal::waker(signal.clone());

    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(value) => break value,
            core::task::Poll::Pending => Signal::wait(&*signal),
        }
    }
}
