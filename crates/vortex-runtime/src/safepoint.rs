// CEP:FILE crates/vortex-runtime/src/safepoint.rs
// CEP:WHAT Global safepoint arbitration (RUNNING/GC_PENDING/GC_ACTIVE/
//        DEOPT_PENDING/DEOPT_ACTIVE with GC precedence) and the generational
//        card table (GC Interaction 6.3, GC design 6.3).
// CEP:WHY Mutators, the GC worker, and the deopt coordinator all poll the
//        same five-state machine; the card table carries old-to-young roots.
//        Isolated here to keep one concept per file and honor 62.5.1.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Arbitration defers requests, never fails; card indices are
//        bounds-checked.
// CEP:ASSUMES Single coordinator instance per runtime.
// CEP:COST O(1) per poll and per dirty; O(cells) per drain.
// CEP:EVIDENCE cargo test -p vortex-runtime --test concurrent_gc
//! Safepoint arbitration and the generational card table.

#![warn(missing_docs)]

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Global safepoint states (GC Interaction 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SafepointState {
    /// No safepoint active.
    Running = 0,
    /// GC requested a safepoint; mutators poll to park.
    GcPending = 1,
    /// GC safepoint active; mutators are parked.
    GcActive = 2,
    /// Deopt requested a safepoint and GC is not active.
    DeoptPending = 3,
    /// Deopt safepoint active; mutators are parked.
    DeoptActive = 4,
}

impl SafepointState {
    fn from_u8(v: u8) -> SafepointState {
        match v {
            0 => SafepointState::Running,
            1 => SafepointState::GcPending,
            2 => SafepointState::GcActive,
            3 => SafepointState::DeoptPending,
            _ => SafepointState::DeoptActive,
        }
    }
}

/// What a mutator must do at its safepoint poll site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafepointAction {
    /// No safepoint: continue executing.
    Continue,
    /// Park until the GC safepoint completes.
    ParkGc,
    /// Park until this thread's deopt request is coordinated.
    ParkDeopt,
    /// A pending deopt request was deferred by GC arbitration; park for
    /// the GC safepoint first, then re-issue the request.
    DeferredToGc,
}

/// The global safepoint coordinator. GC safepoints take strict precedence
/// over deopt safepoints (GC ≻ DEOPT).
// CEP:WHAT Serializes GC and deopt safepoints with GC priority.
// CEP:WHY Concurrent GC and deopt otherwise race on thread state.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; requests during an active safepoint defer.
// CEP:ASSUMES None.
// CEP:COST O(1) per request and per poll.
// CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
pub struct SafepointCoordinator {
    pub(crate) state: AtomicU8,
    /// Mutators currently parked at a safepoint.
    pub(crate) parked: AtomicUsize,
    /// Deopt requests deferred by GC arbitration.
    pub(crate) deferred_deopts: AtomicUsize,
}

impl Default for SafepointCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl SafepointCoordinator {
    /// Creates a coordinator in the Running state.
    // CEP:WHAT Initializes the state machine.
    // CEP:WHY Execution starts outside any safepoint.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn new() -> SafepointCoordinator {
        SafepointCoordinator {
            state: AtomicU8::new(SafepointState::Running as u8),
            parked: AtomicUsize::new(0),
            deferred_deopts: AtomicUsize::new(0),
        }
    }

    /// The current global state.
    // CEP:WHAT Reads the global state.
    // CEP:WHY Workers and tests observe phase transitions.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn state(&self) -> SafepointState {
        SafepointState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Requests a GC safepoint. Any pending or active deopt safepoint is
    /// displaced (its requester must re-issue later).
    // CEP:WHAT Forces the state to GcPending from any state.
    // CEP:WHY Arbitration rule GC ≻ DEOPT (6.3).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn request_gc(&self) {
        let prev = self
            .state
            .swap(SafepointState::GcPending as u8, Ordering::AcqRel);
        if prev == SafepointState::DeoptPending as u8 || prev == SafepointState::DeoptActive as u8 {
            self.deferred_deopts.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Requests a deopt safepoint. Fails (deferred) if a GC safepoint is
    /// pending or active.
    // CEP:WHAT Transitions Running -> DeoptPending when GC allows.
    // CEP:WHY Deopt must never preempt an in-flight GC safepoint.
    // CEP:STATUS complete
    // CEP:FAILURE Returns false when GC arbitration defers the request.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn request_deopt(&self) -> bool {
        let granted = self
            .state
            .compare_exchange(
                SafepointState::Running as u8,
                SafepointState::DeoptPending as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok();
        if !granted && self.state() == SafepointState::GcPending
            || !granted && self.state() == SafepointState::GcActive
        {
            self.deferred_deopts.fetch_add(1, Ordering::Relaxed);
        }
        granted
    }

    /// Activates the GC safepoint (all mutators at the poll site park).
    // CEP:WHAT Transitions GcPending -> GcActive.
    // CEP:WHY The worker runs STW phases only under GcActive.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The caller observed GcPending.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn activate_gc(&self) {
        let _ = self.state.compare_exchange(
            SafepointState::GcPending as u8,
            SafepointState::GcActive as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Ends the GC safepoint and returns to Running.
    // CEP:WHAT Transitions GcActive -> Running.
    // CEP:WHY Parked mutators resume on Running.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The caller observed GcActive.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn finish_gc(&self) {
        let _ = self.state.compare_exchange(
            SafepointState::GcActive as u8,
            SafepointState::Running as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Activates and then ends a deopt safepoint when arbitration allowed
    /// the request.
    // CEP:WHAT Runs the deopt safepoint window.
    // CEP:WHY State reconstruction is atomic to the GC (6.4).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES state is DeoptPending or Running.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn run_deopt_window(&self) {
        let _ = self.state.compare_exchange(
            SafepointState::DeoptPending as u8,
            SafepointState::DeoptActive as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.state
            .store(SafepointState::Running as u8, Ordering::Release);
    }

    /// The mutator-side poll (GC Interaction 6.3 safepoint_poll).
    // CEP:WHAT Classifies what a mutator must do right now.
    // CEP:WHY Every poll site uses exactly this decision.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn poll(&self) -> SafepointAction {
        match self.state() {
            SafepointState::Running => SafepointAction::Continue,
            SafepointState::GcPending | SafepointState::GcActive => SafepointAction::ParkGc,
            SafepointState::DeoptPending | SafepointState::DeoptActive => {
                SafepointAction::ParkDeopt
            }
        }
    }

    /// Number of mutators parked right now.
    // CEP:WHAT Reports the parked-mutator count.
    // CEP:WHY GC waits for quiescence before STW phases.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn parked(&self) -> usize {
        self.parked.load(Ordering::Acquire)
    }

    /// Deopt requests deferred by GC arbitration so far.
    // CEP:WHAT Reports the deferral counter.
    // CEP:WHY Tests verify the arbitration rule fired.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn deferred_deopts(&self) -> usize {
        self.deferred_deopts.load(Ordering::Acquire)
    }
}

/// Dirty cards for the generational invariant: an old-to-young store
/// dirties the card of the source slot; a young collection scans only
/// dirty cards for roots.
// CEP:WHAT Tracks which 256-byte cards hold old-to-young stores.
// CEP:WHY Young collects must find old roots without scanning the heap.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Card indices stay inside the table.
// CEP:COST O(1) per store; O(cards) per scan.
// CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
pub struct CardTable {
    cells: Vec<AtomicU8>,
}

/// The DIRTY cell value.
const CARD_DIRTY: u8 = 1;
/// Bytes of heap covered by one card.
pub const CARD_SIZE: usize = 256;

impl CardTable {
    /// Creates a table covering `heap_bytes` of heap.
    // CEP:WHAT Allocates one atomic cell per card.
    // CEP:WHY Mutators dirty cells lock-free during stores.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(heap_bytes / CARD_SIZE).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn new(heap_bytes: usize) -> CardTable {
        let cells = heap_bytes.div_ceil(CARD_SIZE);
        CardTable {
            cells: (0..cells).map(|_| AtomicU8::new(0)).collect(),
        }
    }

    /// Dirties the card containing `offset`.
    // CEP:WHAT Marks the card of a store source as dirty.
    // CEP:WHY The write barrier calls this on old-to-young stores.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn dirty(&self, offset: usize) {
        let idx = offset / CARD_SIZE;
        if idx < self.cells.len() {
            self.cells[idx].store(CARD_DIRTY, Ordering::Release);
        }
    }

    /// Whether the card containing `offset` is dirty.
    // CEP:WHAT Queries one card.
    // CEP:WHY Tests and the young collector inspect cards.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn is_dirty(&self, offset: usize) -> bool {
        let idx = offset / CARD_SIZE;
        idx < self.cells.len() && self.cells[idx].load(Ordering::Acquire) == CARD_DIRTY
    }

    /// Drains all dirty card indices, cleaning the table.
    // CEP:WHAT Returns dirty cards and resets them.
    // CEP:WHY A young collect scans exactly the returned cards.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(cells).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn take_dirty(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for (idx, cell) in self.cells.iter().enumerate() {
            if cell.swap(0, Ordering::AcqRel) == CARD_DIRTY {
                out.push(idx);
            }
        }
        out
    }
}
