// #![warn(missing_docs)]
// #![warn(clippy::missing_safety_doc)]
#![no_std]

extern crate alloc;
extern crate core;
#[cfg(feature = "std")]
extern crate std;

mod ctx;
pub use ctx::TaskContext;

#[cfg(feature = "std")]
mod std_impl;

mod signal;
pub use signal::Signal;

pub mod com;
pub mod error;

mod task;
pub use task::*;

#[cfg(feature = "global")]
pub mod global;
#[cfg(feature = "global")]
pub use global::GlobalContext;

#[cfg(feature = "std")]
pub use std_impl::{Context as StdContext, Signal as StdSignal};
