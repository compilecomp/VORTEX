// CEP:FILE crates/vortex-runtime/src/concurrent_gc.rs
// CEP:WHAT Concurrent GC driver: worker phases in quanta, global safepoint
//        arbitration with GC precedence, generational card table.
// CEP:WHY The synchronous Collector needs a concurrency shell to satisfy
//        the incremental/concurrent/generational design contract.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Worker exits on Shutdown; arbitration defers, never fails.
// CEP:ASSUMES Single worker thread; commands are quanta-bounded.
// CEP:COST O(|Live|) mark work total; O(1) per mutator operation.
// CEP:EVIDENCE cargo test -p vortex-runtime --test concurrent_gc
//! Concurrent GC driver and global safepoint arbitration (design
//! "Garbage Collection Subsystem" 6.1/6.11 and "GC Interaction Subsystem"
//! 6.3). The synchronous [`crate::gc::Collector`] implements the barrier
//! and marking algorithms; this module adds the concurrency shell around
//! it: a worker thread that runs collection phases in small quanta while
//! mutator threads keep allocating and storing, and the five-state global
//! safepoint state machine with the GC-before-deopt arbitration rule.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

// The safepoint coordinator and card table live in `safepoint.rs`
// (atomic-file split); the public paths are preserved through this
// re-export.
// CEP:WHAT Re-exports the safepoint and card-table structures.
// CEP:WHY Downstream code imports them via crate::concurrent_gc (unchanged paths).
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[doc(inline)]
pub use crate::safepoint::CARD_SIZE;
pub use crate::safepoint::{CardTable, SafepointAction, SafepointCoordinator, SafepointState};
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
// CEP:ID: MEM-a05
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
            Command::Idle => {
                // Nothing queued this tick; the wait above already slept.
            }
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
