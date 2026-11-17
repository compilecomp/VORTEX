// CEP:FILE crates/vortex-t0/src/lib.rs
// CEP:WHAT Crate root for the T0 tier: the speculative register
//        interpreter and its opcode rewriter.
// CEP:WHY T0 is the initial execution tier of the VORTEX engine.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-t0
//! T0: Speculative Register Interpreter.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod frames;
pub mod interp;
pub mod rewrite;

pub use interp::{
    CacheEntry, CacheState, DeoptState, Dst, Frame, HotEvent, InlineCache, Interp, Outcome,
    ProfileSlots, Thresholds,
};
pub use rewrite::{RewritePolicy, Rewriter};
