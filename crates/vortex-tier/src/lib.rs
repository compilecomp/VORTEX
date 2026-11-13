//! vortex-tier: the tiering controller and execution-support subsystems.
// CEP:FILE crates/vortex-tier/src/lib.rs
// CEP:WHAT Crate root: the tier controller (triggers, OSR bookkeeping,
//        deopt storm prevention), the CODE-1 code cache model, the IC-1
//        inline-cache state machine, the EXC-1 exception contract, the
//        MEM-1 memory-order model, and the DEBUG-1 tooling interfaces.
// CEP:WHY These subsystems are where the compiled tiers meet the runtime:
//        the specified contracts are the correctness envelope for every
//        tier transition, cache lifecycle, cache evolution, and unwind.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs; every failure is a typed value.
// CEP:ASSUMES The reference models the subsystems in-process (code images
//        are byte buffers; no mmap), keeping every observable protocol the
//        specified contract.
// CEP:COST Library crate; all queries are O(1) to O(log n).
// CEP:EVIDENCE cargo test -p vortex-tier

/// The CODE-1 code cache model.
pub mod cache;
/// The tiering controller.
pub mod controller;
/// The DEBUG-1 tooling interface model.
pub mod debug;
/// The EXC-1 exception contract model.
pub mod exc;
/// The IC-1 inline-cache state machine.
pub mod ic;
/// The MEM-1 memory-order model.
pub mod mem;

pub use cache::{Arena, BlobState, CacheError, CodeBlobHeader, CodeCache};
pub use controller::{Thresholds, Tier, TierController};
pub use debug::{perf_map, DebugInfo, LocationEntry};
pub use exc::{DeoptTransfer, ExnError, ExnRecord, ExnTable};
pub use ic::{IcSite, IcState, POLY_LIMIT};
pub use mem::{eliminate_redundant_fences, lower, reorder_legal, Lowering, MemoryOrder, Target};
