use crate::{com, std_impl};
use core::sync::atomic::{AtomicUsize, Ordering};

pub type Signal = std_impl::Signal;

static INITIAL_WORKERS: AtomicUsize = AtomicUsize::new(0);

pub struct GlobalContext {
    imp: std_impl::Context,
}

static GLOBAL: spin::Lazy<GlobalContext> = spin::Lazy::new(|| GlobalContext {
    imp: crate::signal::block_on_signal(
        Signal::new(),
        std_impl::Context::new({
            let workers = INITIAL_WORKERS.load(Ordering::Acquire);
            if workers == 0 {
                num_cpus::get()
            } else {
                workers
            }
        }),
    ),
});

/// Only works if called before anything uses the Global context.
/// Setting the amount to zero will cause it to pick the amount of cores
/// that are available to the system as a default.
pub fn init_with_limit(limit: usize) {
    INITIAL_WORKERS.store(limit, Ordering::Release);
    spin::Lazy::force(&GLOBAL);
}

impl GlobalContext {
    pub fn get() -> &'static Self {
        &GLOBAL
    }

    pub fn signal() -> Signal {
        Signal::new()
    }
}

impl crate::TaskContext for GlobalContext {
    async fn set_limit(&self, max: usize) {
        self.imp.set_limit(max).await
    }

    async fn create_task(&self, f: com::TaskFn) -> com::ComHandle {
        self.imp.create_task(f).await
    }
}
