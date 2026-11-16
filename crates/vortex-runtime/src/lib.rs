// CEP:FILE crates/vortex-runtime/src/lib.rs
// CEP:WHAT Crate root for the VORTEX runtime: trap taxonomy, capability
//        system, heap object model, garbage collector, profiling model.
// CEP:WHY The interpreter and the tier controller share one runtime; this
//        crate owns every runtime failure mode and memory policy.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-runtime
//! VORTEX runtime: traps, capabilities, heap, GC, profiling.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod caps;
pub mod gc;
pub mod heap;
pub mod profile;
pub mod trap;

pub use caps::{CapError, Environment, Gct, Status, Token, GCT_INVALID, MAX_CAP_SLOTS};
pub use gc::{
    Collector, Colors, ForwardingCache, GcConfig, GcStats, MarkBitmap, PointerColor, SatbQueue,
};
pub use heap::{
    Heap, HeapConfig, Region, RegionKind, Tlab, CARD_SIZE, FLAG_FORWARDED, HEADER_SIZE,
};
pub use profile::{ProfileData, ValueType};
pub use trap::{RtResult, Trap};
