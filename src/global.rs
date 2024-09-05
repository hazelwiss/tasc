use crate::{com, std_impl};
use core::sync::atomic::{AtomicUsize, Ordering};

/// The signal type used by the global context.
pub type Signal = std_impl::Signal;

static INITIAL_WORKERS: AtomicUsize = AtomicUsize::new(0);

/// The global context, which is used in `[ThreadBuilder::default]`, `[tasc::task]`, `[tasc::scope]`, `[tasc::blocking::task]` and `[tasc::blocking::scope]`.
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
    #[allow(missing_docs)]
    pub fn get() -> &'static Self {
        &GLOBAL
    }

    /// Creates a signal which is the same signal used by the global context. This is the same as using
    /// `[StdSignal::new]`
    pub fn signal() -> Signal {
        Signal::new()
    }
}

impl crate::TaskContext for GlobalContext {
    async fn set_workers(&self, max: usize) {
        self.imp.set_workers(max).await
    }

    fn workers(&self) -> usize {
        self.imp.workers()
    }

    async fn create_task(&self, f: com::TaskFn) -> com::ComHandle {
        self.imp.create_task(f).await
    }
}
