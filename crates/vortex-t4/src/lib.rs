// CEP:FILE crates/vortex-t4/src/lib.rs
// CEP:WHAT Crate root for the T4 tier: guarded trace recording and the
//        Class-2 compilation pipeline under the tier budget.
// CEP:WHY T4 is the O(n^2) passes tier of the tiered system.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-t4
//! T4: the O(n^2) passes tier.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod compile;
pub mod trace;

pub use compile::{compile_t4, T4Config, T4Stats};
pub use trace::{Assumption, Guard, Trace, TraceRecorder};
