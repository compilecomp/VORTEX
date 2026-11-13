//! T1: the stencil-first baseline compiler (copy-and-patch) with the
//! linear-scan fallback, over the shared T0 semantics.
// CEP:FILE crates/vortex-t1/src/lib.rs
// CEP:WHAT T1 crate root: the T1a/T1b/T1c path classifier, the copy-and-patch
//        compiler (decode, stencil selection, copy, patch), the T1c linear-
//        scan fallback, and the blob executor over the shared T0 semantics.
// CEP:WHY T1 is the latency-critical baseline tier: compilation is a memcpy
//        per stencil plus integer patching, with no IR and no allocator on
//        the stencil paths. The revised structure (design.md section 5) is
//        stencil-first, not stencil-only, so complex methods route to T1c.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs; all errors are typed, none swallowed.
// CEP:ASSUMES Methods pass vortex-ugb verification before compilation.
// CEP:COST Compilation is O(instructions + holes); execution is O(instances).
// CEP:EVIDENCE cargo test -p vortex-t1

/// The copy-and-patch compiler and the T1 blob model.
pub mod copy_patch;
/// The blob executor over the shared T0 semantics.
pub mod exec;
/// The T1c linear-scan fallback.
pub mod fallback;
/// Path classification and the fixed physical map.
pub mod plan;

pub use copy_patch::{compile_baseline, compile_with_plan, fixed_map_plan, T1Blob, T1Error};
pub use exec::{live_vregs_at, T1Machine};
pub use plan::{classify, FixedMap, T1Path, FAST_MAP_REGS, SPILL_BUDGET};
