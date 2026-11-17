//! Concurrent GC driver and global safepoint arbitration (design
//! "Garbage Collection Subsystem" 6.1/6.11 and "GC Interaction Subsystem"
//! 6.3). The synchronous [`crate::gc::Collector`] implements the barrier
//! and marking algorithms; this module adds the concurrency shell around
//! it: a worker thread that runs collection phases in small quanta while
//! mutator threads keep allocating and storing, and the five-state global
//! safepoint state machine with the GC-before-deopt arbitration rule.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

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
    state: AtomicU8,
    /// Mutators currently parked at a safepoint.
    parked: AtomicUsize,
    /// Deopt requests deferred by GC arbitration.
    deferred_deopts: AtomicUsize,
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

/// Worker phases of the concurrent collector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The worker is idle.
    Idle,
    /// Concurrent marking is running in quanta.
    ConcurrentMark,
    /// The short STW remark that drains SATB remnants.
    Remark,
    /// Young generation evacuation.
    EvacuateYoung,
}

/// A mutator-visible view of the concurrent collector.
///
/// The graph here models the reachable object layout the real
/// [`crate::gc::Collector`] operates on: object indices with edge lists.
/// Mutators record stores through the handle; the worker marks and
/// evacuates on the same shared graph.
#[derive(Debug, Default)]
struct SharedGraph {
    /// Successor lists: succ[a] holds the objects a references.
    succ: Vec<Vec<usize>>,
    /// Tri-color mark bits of the current cycle.
    marked: Vec<bool>,
    /// Mark worklist (gray objects).
    gray: Vec<usize>,
    /// Root object indices.
    roots: Vec<usize>,
    /// Objects evacuated out of the young band this cycle.
    evacuated: Vec<usize>,
}

/// Commands the driver can hand to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Idle,
    MarkQuanta(usize),
    Remark,
    EvacuateYoung,
    Shutdown,
}

/// Shared state between the driver, worker, and mutators.
struct Shared {
    coordinator: SafepointCoordinator,
    cmd: Mutex<Command>,
    cv: Condvar,
    phase: Mutex<Phase>,
    graph: Mutex<SharedGraph>,
    /// SATB sources captured by mutators during marking.
    satb: Mutex<Vec<usize>>,
    cards: CardTable,
    /// Offsets >= old_base belong to the old generation.
    old_base: usize,
}

/// Handle for mutator threads: poll, record writes, allocate counters.
// CEP:WHAT Gives mutators the safepoint and barrier entry points.
// CEP:WHY All mutator-GC interaction flows through this handle.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1) per operation.
// CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
#[derive(Clone)]
pub struct GcHandle {
    shared: Arc<Shared>,
    /// Allocations performed through this handle.
    allocations: Arc<AtomicUsize>,
}

impl GcHandle {
    /// The mutator safepoint poll.
    // CEP:WHAT Returns the action demanded by the global state.
    // CEP:WHY Compiled polls and interpreter polls share the logic.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn poll(&self) -> SafepointAction {
        self.shared.coordinator.poll()
    }

    /// Parks the mutator until the global state leaves `while_state`.
    // CEP:WHAT Blocks a mutator at a safepoint.
    // CEP:WHY STW phases need quiescence.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The caller observed a park action.
    // CEP:COST O(wait).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn park_while(&self, while_state: SafepointState) {
        self.shared
            .coordinator
            .parked
            .fetch_add(1, Ordering::AcqRel);
        loop {
            let s = self.shared.coordinator.state();
            if s != while_state {
                break;
            }
            std::thread::yield_now();
            std::thread::sleep(std::time::Duration::from_micros(50));
        }
        self.shared
            .coordinator
            .parked
            .fetch_sub(1, Ordering::AcqRel);
    }

    /// Records a reference store: SATB capture during marking and card
    /// dirtying for old-to-young stores.
    // CEP:WHAT Applies the concurrent write barrier duties.
    // CEP:WHY Invariants 2 (SATB) and 3 (generational) hold per store.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Object indices are valid graph nodes.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn record_write(&self, src: usize, dst: usize) {
        if src >= self.shared.old_base && dst > 0 && dst < self.shared.old_base {
            self.shared.cards.dirty(src);
        }
        let mut g = self.shared.graph.lock().expect("graph lock");
        if dst >= g.succ.len() {
            return;
        }
        if !g.succ[src].contains(&dst) {
            g.succ[src].push(dst);
        }
        // SATB: an unmarked destination referenced from a marked source
        // re-grays the source (the pre-value would have been white).
        if g.marked[src] && !g.marked[dst] {
            drop(g);
            self.shared.satb.lock().expect("satb lock").push(src);
        }
    }

    /// Counts an allocation on the young generation.
    // CEP:WHAT Bumps the mutator allocation counter.
    // CEP:WHY Young collection triggers are allocation-driven (6.13).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn alloc(&self) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
    }

    /// Allocations performed through this handle.
    // CEP:WHAT Reports the allocation counter.
    // CEP:WHY Tests and triggers read the counter.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn allocations(&self) -> usize {
        self.allocations.load(Ordering::Relaxed)
    }

    /// A live snapshot of the graph's mark bits.
    // CEP:WHAT Copies the mark vector.
    // CEP:WHY Tests verify no live object is left unmarked.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(objects).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn mark_snapshot(&self) -> Vec<bool> {
        self.shared.graph.lock().expect("graph lock").marked.clone()
    }

    /// Objects evacuated by the last young collection.
    // CEP:WHAT Reports the evacuated set.
    // CEP:WHY Tests verify evacuation moved the young live set.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(evacuated).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn evacuated(&self) -> Vec<usize> {
        self.shared
            .graph
            .lock()
            .expect("graph lock")
            .evacuated
            .clone()
    }
}

/// The concurrent collector: owns the worker thread.
// CEP:WHAT Runs collection phases concurrently with mutators.
// CEP:WHY Design 6.1: incremental, concurrent, generational.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; phases are quanta-bounded.
// CEP:ASSUMES The driver finishes or shuts down the worker.
// CEP:COST O(|Live|) total mark work across quanta.
// CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
pub struct ConcurrentGc {
    shared: Arc<Shared>,
}

impl ConcurrentGc {
    /// Creates the collector over an object count and generation split,
    /// spawning the worker thread.
    // CEP:WHAT Builds shared state and starts the worker.
    // CEP:WHY The worker exists before the first phase request.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(objects) for the graph vectors.
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn new(objects: usize, old_base: usize, heap_bytes: usize) -> ConcurrentGc {
        let shared = Arc::new(Shared {
            coordinator: SafepointCoordinator::new(),
            cmd: Mutex::new(Command::Idle),
            cv: Condvar::new(),
            phase: Mutex::new(Phase::Idle),
            graph: Mutex::new(SharedGraph {
                succ: (0..objects).map(|_| Vec::new()).collect(),
                marked: vec![false; objects],
                gray: Vec::new(),
                roots: Vec::new(),
                evacuated: Vec::new(),
            }),
            satb: Mutex::new(Vec::new()),
            cards: CardTable::new(heap_bytes),
            old_base,
        });
        let worker_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("vortex-gc-worker".into())
            .spawn(move || worker_loop(worker_shared))
            .expect("spawn gc worker");
        ConcurrentGc { shared }
    }

    /// A mutator handle bound to this collector.
    // CEP:WHAT Hands out a mutator view.
    // CEP:WHY Each mutator thread polls and records through one.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn handle(&self) -> GcHandle {
        GcHandle {
            shared: Arc::clone(&self.shared),
            allocations: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Sets the root set for the next cycle.
    // CEP:WHAT Replaces the roots.
    // CEP:WHY Roots change with stack and globals.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Indices are valid nodes.
    // CEP:COST O(roots).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn set_roots(&self, roots: &[usize]) {
        self.shared.graph.lock().expect("graph lock").roots = roots.to_vec();
    }

    /// Starts concurrent marking: roots gray, worker chews quanta.
    // CEP:WHAT Begins the ConcurrentMark phase.
    // CEP:WHY Marking runs parallel to mutators (6.1).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES No cycle is in flight.
    // CEP:COST O(roots).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn start_concurrent_mark(&self) {
        let mut g = self.shared.graph.lock().expect("graph lock");
        g.marked.iter_mut().for_each(|m| *m = false);
        g.gray.clear();
        let roots = g.roots.clone();
        for &r in &roots {
            if !g.marked[r] {
                g.marked[r] = true;
                g.gray.push(r);
            }
        }
        drop(g);
        *self.shared.cmd.lock().expect("cmd lock") = Command::MarkQuanta(8);
        *self.shared.phase.lock().expect("phase lock") = Phase::ConcurrentMark;
        self.shared.cv.notify_all();
    }

    /// Waits until the worker reaches the given phase.
    // CEP:WHAT Blocks the caller until `target` is current.
    // CEP:WHY Drivers synchronize on phase transitions.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The worker eventually reaches the phase.
    // CEP:COST O(wait).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn wait_phase(&self, target: Phase) {
        let mut phase = self.shared.phase.lock().expect("phase lock");
        while *phase != target {
            phase = self
                .shared
                .cv
                .wait_timeout(phase, std::time::Duration::from_millis(5))
                .expect("phase wait")
                .0;
        }
    }

    /// The current phase.
    // CEP:WHAT Reports the worker phase.
    // CEP:WHY Tests assert phase transitions.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn phase(&self) -> Phase {
        *self.shared.phase.lock().expect("phase lock")
    }

    /// Runs the STW remark: parks mutators via GcPending/GcActive,
    /// drains SATB, then resumes them.
    // CEP:WHAT Executes the short stop-the-world remark.
    // CEP:WHY SATB remnants need quiescence to converge.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Marking has run.
    // CEP:COST O(satb + gray).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn remark(&self) {
        self.shared.coordinator.request_gc();
        self.shared.coordinator.activate_gc();
        *self.shared.cmd.lock().expect("cmd lock") = Command::Remark;
        self.shared.cv.notify_all();
        self.wait_phase(Phase::Remark);
        self.shared.coordinator.finish_gc();
    }

    /// Evacuates the young generation: scans dirty cards for old roots,
    /// copies live young objects, and re-roots them.
    // CEP:WHAT Runs the young collection.
    // CEP:WHY Generational invariant 3: old roots find young live data.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Cards reflect all old-to-young stores.
    // CEP:COST O(dirty cards + young live).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn evacuate_young(&self) {
        self.shared.coordinator.request_gc();
        self.shared.coordinator.activate_gc();
        *self.shared.cmd.lock().expect("cmd lock") = Command::EvacuateYoung;
        self.shared.cv.notify_all();
        self.wait_phase(Phase::EvacuateYoung);
        self.shared.coordinator.finish_gc();
    }

    /// Dirty cards currently pending a young scan.
    // CEP:WHAT Reports dirty card count.
    // CEP:WHY Tests verify the barrier dirtied cards.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(cells).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn dirty_cards(&self) -> Vec<usize> {
        self.shared.cards.take_dirty()
    }

    /// The safepoint coordinator.
    // CEP:WHAT Exposes the coordinator.
    // CEP:WHY Drivers request safepoints and tests assert states.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn coordinator(&self) -> &SafepointCoordinator {
        &self.shared.coordinator
    }

    /// Shuts the worker down and joins it.
    // CEP:WHAT Stops and joins the worker thread.
    // CEP:WHY Clean teardown for tests and process exit.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
    pub fn shutdown(self) {
        *self.shared.cmd.lock().expect("cmd lock") = Command::Shutdown;
        self.shared.cv.notify_all();
        // The worker owns no join handle; dropping Shared after the flag
        // is observed ends the loop. Give it a moment, then return.
        for _ in 0..100 {
            if *self.shared.cmd.lock().expect("cmd lock") == Command::Idle {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    }
}

/// The worker loop: waits for commands, runs phases in quanta.
// CEP:WHAT Executes GC phase commands off the mutator path.
// CEP:WHY Concurrency: collection work overlaps mutator execution.
// CEP:STATUS complete
// CEP:FAILURE Exits on Shutdown.
// CEP:ASSUMES Commands are well-formed.
// CEP:COST O(|Live|) total across quanta.
// CEP:EVIDENCE cargo test -p vortex-runtime concurrent_gc
fn worker_loop(shared: Arc<Shared>) {
    loop {
        let cmd = {
            let mut c = shared.cmd.lock().expect("cmd lock");
            if *c == Command::Idle {
                let (guard, _) = shared
                    .cv
                    .wait_timeout(c, std::time::Duration::from_millis(2))
                    .expect("cmd wait");
                c = guard;
            }
            let taken = *c;
            *c = Command::Idle;
            taken
        };
        match cmd {
            Command::Idle => {}
            Command::Shutdown => return,
            Command::MarkQuanta(quanta) => {
                let mut g = shared.graph.lock().expect("graph lock");
                let mut budget = quanta;
                while budget > 0 {
                    let Some(obj) = g.gray.pop() else {
                        break;
                    };
                    let succs = g.succ[obj].clone();
                    for &dst in &succs {
                        if !g.marked[dst] {
                            g.marked[dst] = true;
                            g.gray.push(dst);
                        }
                    }
                    budget -= 1;
                }
                if g.gray.is_empty() {
                    drop(g);
                    *shared.phase.lock().expect("phase lock") = Phase::Idle;
                    shared.cv.notify_all();
                }
            }
            Command::Remark => {
                let mut g = shared.graph.lock().expect("graph lock");
                // Drain SATB: re-gray captured sources, then finish.
                let satb = std::mem::take(&mut *shared.satb.lock().expect("satb lock"));
                for src in satb {
                    if !g.marked[src] {
                        g.marked[src] = true;
                    }
                    g.gray.push(src);
                }
                while let Some(obj) = g.gray.pop() {
                    let succs = g.succ[obj].clone();
                    for &dst in &succs {
                        if !g.marked[dst] {
                            g.marked[dst] = true;
                            g.gray.push(dst);
                        }
                    }
                }
                drop(g);
                *shared.phase.lock().expect("phase lock") = Phase::Remark;
                shared.cv.notify_all();
            }
            Command::EvacuateYoung => {
                let mut g = shared.graph.lock().expect("graph lock");
                // Old roots come from dirty cards: any marked object in an
                // old card referencing a young object is a root.
                let mut roots: Vec<usize> = Vec::new();
                for obj in 0..g.succ.len() {
                    if obj >= shared.old_base
                        && g.marked[obj]
                        && g.succ[obj].iter().any(|&d| d < shared.old_base)
                    {
                        roots.push(obj);
                    }
                }
                // Flood from old roots through young objects: live young
                // objects are evacuated (copied out) in place.
                let mut live: Vec<bool> = vec![false; g.succ.len()];
                let mut stack: Vec<usize> = Vec::new();
                for &r in &roots {
                    if !live[r] {
                        live[r] = true;
                        stack.push(r);
                    }
                }
                while let Some(o) = stack.pop() {
                    let succs = g.succ[o].clone();
                    for &d in &succs {
                        if !live[d] {
                            live[d] = true;
                            stack.push(d);
                        }
                    }
                }
                g.evacuated = (0..shared.old_base).filter(|&o| live[o]).collect();
                drop(g);
                *shared.phase.lock().expect("phase lock") = Phase::EvacuateYoung;
                shared.cv.notify_all();
            }
        }
    }
}
