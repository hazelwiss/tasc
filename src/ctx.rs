use core::future::Future;

use crate::com;

pub trait TaskContext {
    /// The limit can only ever increase.
    /// If you set the limit, then set it again to a lower limit,
    /// the limit will not change.
    fn set_limit(&self, max: usize) -> impl Future<Output = ()>;

    fn create_task(&self, f: com::TaskFn) -> impl Future<Output = com::ComHandle>;
}
