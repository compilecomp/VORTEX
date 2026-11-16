// CEP:FILE crates/vortex-t1/src/lib.rs
// CEP:WHAT Crate root for the T1 baseline tier: stencil planning, patched
//        execution, and T0 fallback classification.
// CEP:WHY T1 is the copy-and-patch baseline JIT consuming UGB.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-t1
//! T1: Copy-and-Patch Baseline Tier.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod exec;
pub mod fallback;
pub mod plan;

pub use exec::{Dst, Executor};
pub use fallback::{classify, Coverage};
pub use plan::{plan_method, CodeObject, PatchedInstr, PlanError};
