use alloc::sync::Arc;
use core::{future::Future, task::Context};

pub trait Signal {
    fn wait(&self);

    fn waker(self: Arc<Self>) -> core::task::Waker;
}

#[allow(unused)]
pub fn noop() -> impl Signal {
    struct Noop;

    impl Noop {
        fn raw() -> core::task::RawWaker {
            core::task::RawWaker::new(
                core::ptr::null(),
                &core::task::RawWakerVTable::new(|_| Self::raw(), |_| {}, |_| {}, |_| {}),
            )
        }
    }

    impl Signal for Noop {
        fn wait(&self) {}

        fn waker(self: Arc<Self>) -> core::task::Waker {
            unsafe { core::task::Waker::from_raw(Self::raw()) }
        }
    }

    Noop
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    #[cfg(feature = "std")]
    let signal = crate::std_impl::Signal::new();
    #[cfg(not(feature = "std"))]
    let signal = noop();

    block_on_with_signal(signal, future)
}

pub fn block_on_with_signal<F: Future>(signal: impl Signal, mut future: F) -> F::Output {
    let mut future = unsafe { std::pin::Pin::new_unchecked(&mut future) };

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
