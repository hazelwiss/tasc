use core::future::Future;

use crate::com;

/// The trait implemented by the context.
///
/// This trait is used by `[TaskBuilder]` to create new tasks, as well as to increase the capacity of workers.
/// This trait is not object safe.
pub trait TaskContext {
    /// The limit can only ever increase.
    /// If you set the limit, then set it again to a lower limit,
    /// the limit will not change.
    fn set_limit(&self, max: usize) -> impl Future<Output = ()>;

    /// Creates an asynchronous task used by handle traits inside of `task`.
    fn create_task(&self, f: com::TaskFn) -> impl Future<Output = com::ComHandle>;
}
