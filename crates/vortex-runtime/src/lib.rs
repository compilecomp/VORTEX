//! VORTEX runtime: heap, generational GC with pinned snapshots, AOSS,
//! capability epochs, dependency handshake, profiling, stencils.
// CEP:FILE crates/vortex-runtime/src/lib.rs
// CEP:WHAT Runtime crate root: heap, GC, capabilities, dependency map,
//        profiling, stencils, and AOSS, re-exported as one coherent API.
// CEP:WHY The runtime is the correctness anchor of the engine; the
//        interpreter (vortex-t0) composes these modules.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES Single-mutator execution for heap access; dependency-map
//        concurrency is exercised through deps.rs directly.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-runtime

/// AOSS: layout registry and numeric-array transitions (NUM-1).
pub mod aoss;
/// Capability table with 16-bit version epochs (UGB-1 6.3).
pub mod caps;
/// Dependency map with the add-vs-invalidate handshake (CONC-1 2.1.1).
pub mod deps;
/// Generational collector with pinned snapshots (SNAP-1 6.3).
pub mod gc;
/// Paged heap, barriers, and object model (HEAP-1 revised).
pub mod heap;
/// Profile store with sampled back-edge counters (PROF-1 3.2).
pub mod profile;
/// T1 stencil pool and OSR trampoline stubs (OSR-1 6.1).
pub mod stencil;
/// Typed trap taxonomy.
pub mod trap;

pub use aoss::{Aoss, Decision, LayoutDescriptor, LayoutRegistry, HOMOGENEITY_WINDOW};
pub use caps::{auth, CapId, CapabilityTable};
pub use deps::{DepError, DependencyMap};
pub use gc::{Gc, Roots};
pub use heap::{
    layout, GcStats, Handle, Heap, Page, PageKind, GC_FORWARDED, GC_STATE_MASK, GC_VALID,
    HEADER_WORDS, WORD,
};
pub use profile::{EpochCache, LoopCounter, ProfileConfig, ProfileStore};
pub use stencil::{
    CodeBlob, Loc, OsrCounter, OsrTrampolineStub, PayloadHole, PayloadKind, Stencil, StencilKey,
    StencilPool,
};
pub use trap::{RtResult, Trap};

/// Convenience bundle the interpreter drives: heap, GC, capabilities,
/// profile, AOSS, and the remembered set.
#[derive(Debug, Default)]
pub struct Machine {
    /// The paged heap.
    pub heap: Heap,
    /// The collector and its counters.
    pub gc: Gc,
    /// Capability table with version epochs.
    pub caps: CapabilityTable,
    /// Loop profile store (sampled back-edge counters, T4 requests).
    pub profile: ProfileStore,
    /// AOSS subsystem (registry + homogeneity observers).
    pub aoss: Aoss,
    /// Generational remembered set (old arrays that may point young).
    pub remembered: Vec<Handle>,
    /// Pending exception word (thrown but not yet caught).
    pub pending_exception: Option<u64>,
    /// Capability-site epoch caches keyed by (method, site), maintained by
    /// the interpreter (UGB-1 section 6.3 fast path).
    pub site_epochs: std::collections::HashMap<(usize, usize), Option<u16>>,
    /// Call-site counters keyed by (method, pc), maintained by the
    /// interpreter (design.md section 2.3: call-site counters trigger
    /// tier-up). Saturation-safe: counters cap at u64::MAX.
    pub call_counts: std::collections::HashMap<(usize, usize), u64>,
    /// Maximum interpreter frame-stack depth (bounded failure for recursive
    /// programs; the reference bound is 256 frames).
    pub call_depth_bound: usize,
}

impl Machine {
    /// Creates a machine with one nursery and one old page.
    #[must_use]
    pub fn new() -> Self {
        let mut m = Machine::default();
        m.heap.add_nursery_page();
        m.heap.add_old_page();
        m.call_depth_bound = 256;
        m
    }
}
