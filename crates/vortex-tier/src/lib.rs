// CEP:FILE crates/vortex-tier/src/lib.rs
// CEP:WHAT Crate root for the tier controller: the tier state machine
//        with worker threads, and the concurrent coordination
//        primitives (safepoints, invalidation).
// CEP:WHY vortex-tier orchestrates every tier transition, cancellation,
//        and deopt demotion across threads.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-tier
//! VORTEX tier controller.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod concurrent;
pub mod controller;

pub use concurrent::{Invalidation, InvalidationMap, SafepointGate};
pub use controller::{Controller, FunctionState, ReqState, Request, Tier};
