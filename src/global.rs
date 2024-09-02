use crate::{com, std_impl};

pub struct GlobalContext {
    #[cfg(feature = "std")]
    imp: std_impl::Context,
}

static GLOBAL: spin::Lazy<GlobalContext> = spin::Lazy::new(|| {
    #[cfg(feature = "std")]
    return GlobalContext {
        imp: crate::waker::block_on(std_impl::Context::new(20)),
    };
    #[cfg(not(feature = "std"))]
    panic!("tried to initialize global lazily without the std feature")
});

#[cfg(feature = "std")]
mod imp {
    use super::*;

    impl GlobalContext {
        pub fn get() -> &'static Self {
            &GLOBAL
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
}
