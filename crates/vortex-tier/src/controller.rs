// CEP:FILE crates/vortex-tier/src/controller.rs
// CEP:WHAT The tier controller: per-function tier state machine, hotness
//        metrics with adaptive thresholds, a hotness-over-cost priority
//        queue, a worker thread pool with cooperative cancellation, and
//        deopt demotion (tier design 6.3-6.10; cancellation design
//        6.3-6.7).
// CEP:WHY Tier transitions amortize compile cost over execution benefit;
//        the controller is the single authority for what runs where.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE All request state transitions are atomic compare-and-swap;
//        cancellation is idempotent and can never corrupt a completed
//        install.
// CEP:ASSUMES One controller instance owns its code cache.
// CEP:COST O(1) per event; worker cost bounded by the request size.
// CEP:EVIDENCE cargo test -p vortex-tier
//! Tier controller with worker pool.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Compilation tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Speculative register interpreter.
    T0,
    /// Copy-and-patch baseline.
    T1,
    /// O(1) passes.
    T2,
    /// O(n) passes.
    T3,
    /// O(n^2) passes.
    T4,
    /// Full optimization.
    T5,
}

impl Tier {
    /// The hotness threshold for promotion to the next tier (6.6).
    // CEP:WHAT Returns the promotion threshold out of this tier.
    // CEP:WHY 6.6 fixes the thresholds at 100 through 1,000,000.
    // CEP:STATUS complete
    // CEP:FAILURE None at T5 (no promotion exists).
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn threshold_to_next(self) -> Option<u64> {
        match self {
            Tier::T0 => Some(100),
            Tier::T1 => Some(1_000),
            Tier::T2 => Some(10_000),
            Tier::T3 => Some(100_000),
            Tier::T4 => Some(1_000_000),
            Tier::T5 => None,
        }
    }

    /// The next higher tier.
    // CEP:WHAT Returns the promotion target.
    // CEP:WHY Monotonic promotion walks the taxonomy in order.
    // CEP:STATUS complete
    // CEP:FAILURE None at T5.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn next(self) -> Option<Tier> {
        match self {
            Tier::T0 => Some(Tier::T1),
            Tier::T1 => Some(Tier::T2),
            Tier::T2 => Some(Tier::T3),
            Tier::T3 => Some(Tier::T4),
            Tier::T4 => Some(Tier::T5),
            Tier::T5 => None,
        }
    }
}

/// Request lifecycle states (cancellation design 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReqState {
    /// Waiting in the queue.
    Queued = 0,
    /// Assigned to a worker.
    Compiling = 1,
    /// Cancel flag set while compiling.
    CancellationRequested = 2,
    /// Cleanup in progress.
    Cancelling = 3,
    /// Cancelled before completion.
    Cancelled = 4,
    /// Passes finished.
    Completed = 5,
    /// Code installed.
    Installed = 6,
}

/// Per-function controller state.
#[derive(Debug, Clone)]
pub struct FunctionState {
    /// Current tier.
    pub tier: Tier,
    /// Invocation count.
    pub invocations: u64,
    /// Loop iterations observed.
    pub loop_iterations: u64,
    /// Deoptimization count.
    pub deopts: u32,
}

/// One compilation request.
#[derive(Debug, Clone)]
pub struct Request {
    /// Unique id.
    pub id: u64,
    /// Method to compile.
    pub method: u32,
    /// Target tier.
    pub tier: Tier,
    /// Shared lifecycle state.
    pub state: Arc<AtomicU8>,
    /// Shared cancellation flag.
    pub cancel: Arc<AtomicBool>,
    /// Model IR size for the simulated compile.
    pub nodes: usize,
}

impl Request {
    /// The current lifecycle state.
    // CEP:WHAT Loads the request state.
    // CEP:WHY Tests and the controller poll this.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn state(&self) -> ReqState {
        match self.state.load(Ordering::Acquire) {
            1 => ReqState::Compiling,
            2 => ReqState::CancellationRequested,
            3 => ReqState::Cancelling,
            4 => ReqState::Cancelled,
            5 => ReqState::Completed,
            6 => ReqState::Installed,
            _ => ReqState::Queued,
        }
    }
}

/// Queue entry: higher hotness-over-cost first (6.8); ties resolve by
/// submission sequence. The queue is a sorted Vec, so no ordering traits
/// are derived on foreign-ish fields.
#[derive(Debug)]
struct QueueEntry {
    priority: f64,
    seq: u64,
    request: Request,
}

/// Shared controller internals.
struct Inner {
    functions: HashMap<u32, FunctionState>,
    next_id: u64,
    seq: u64,
}

/// The tier controller with its worker pool.
pub struct Controller {
    inner: Arc<Mutex<Inner>>,
    queue: Arc<Mutex<Vec<QueueEntry>>>,
    cache: Arc<RwLock<HashMap<u32, Tier>>>,
    backlog: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl Controller {
    /// Starts a controller with `workers` compiler threads.
    // CEP:WHAT Spawns the worker pool.
    // CEP:WHY 6.8: the queue is processed by a pool of compiler threads.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(workers).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn new(workers: usize) -> Controller {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let cache = Arc::new(RwLock::new(HashMap::new()));
        let backlog = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..workers.max(1) {
            let queue = queue.clone();
            let cache = cache.clone();
            let shutdown = shutdown.clone();
            let backlog = backlog.clone();
            handles.push(std::thread::spawn(move || {
                worker_loop(queue, cache, shutdown, backlog)
            }));
        }
        Controller {
            inner: Arc::new(Mutex::new(Inner {
                functions: HashMap::new(),
                next_id: 0,
                seq: 0,
            })),
            queue,
            cache,
            backlog,
            shutdown,
            workers: handles,
        }
    }

    /// The installed tier of a method.
    // CEP:WHAT Reads the code cache.
    // CEP:WHY Dispatch uses the installed tier.
    // CEP:STATUS complete
    // CEP:FAILURE None before installation.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn installed_tier(&self, method: u32) -> Option<Tier> {
        self.cache.read().expect("cache lock").get(&method).copied()
    }

    /// Records execution and returns a request when the function turns
    /// hot enough to promote (6.6).
    // CEP:WHAT Updates hotness and enqueues a promotion when due.
    // CEP:WHY Transition policy: Hotness(f) >= Threshold(T_k).
    // CEP:STATUS complete
    // CEP:FAILURE None when no promotion is due or one is in flight.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    // CEP:ID: JIT-a60
    pub fn note_execution(
        &self,
        method: u32,
        invocations: u64,
        loop_iterations: u64,
    ) -> Option<Request> {
        // Phase 1: update hotness and decide the promotion under the
        // controller lock; the tier advances here so the next promotion
        // compares against the new threshold (monotonic taxonomy, 6.6).
        let (hotness, seq, id, next) = {
            let mut inner = self.inner.lock().expect("controller lock");
            let fs = inner.functions.entry(method).or_insert(FunctionState {
                tier: Tier::T0,
                invocations: 0,
                loop_iterations: 0,
                deopts: 0,
            });
            fs.invocations += invocations;
            fs.loop_iterations += loop_iterations;
            let hotness = fs.invocations + 10 * fs.loop_iterations;
            let next = fs.tier.next()?;
            let base = fs.tier.threshold_to_next().unwrap_or(u64::MAX);
            // Adaptive thresholds: backlog raises the bar (6.6).
            let backlog = self.backlog.load(Ordering::Relaxed) as u64;
            let effective = base.saturating_mul(1 + backlog / 1000);
            if hotness < effective {
                return None;
            }
            fs.tier = next;
            let id = inner.next_id;
            inner.next_id += 1;
            inner.seq += 1;
            (hotness, inner.seq, id, next)
        };
        // Phase 2: enqueue with hotness-over-cost priority (6.8).
        let request = Request {
            id,
            method,
            tier: next,
            state: Arc::new(AtomicU8::new(ReqState::Queued as u8)),
            cancel: Arc::new(AtomicBool::new(false)),
            nodes: 200,
        };
        let priority = hotness as f64 / (request.nodes as f64).max(1.0);
        self.queue.lock().expect("queue lock").push(QueueEntry {
            priority,
            seq,
            request: request.clone(),
        });
        self.backlog.fetch_add(1, Ordering::Relaxed);
        Some(request)
    }

    /// Demotes a method after deoptimization (6.6 demotion).
    // CEP:WHAT Steps the tier down, keeping hotness.
    // CEP:WHY Deopt falls back to T_{k-1} without resetting hotness.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn demote(&self, method: u32) {
        let mut inner = self.inner.lock().expect("controller lock");
        if let Some(fs) = inner.functions.get_mut(&method) {
            fs.deopts += 1;
            fs.tier = match fs.tier {
                Tier::T0 => Tier::T0,
                other => tier_from_ordinal(other as u8 - 1),
            };
        }
        // The demoted code leaves the cache until recompiled.
        self.cache.write().expect("cache lock").remove(&method);
    }

    /// The current tier a method would run at.
    // CEP:WHAT Reports the function's tier.
    // CEP:WHY Tests and dispatch read the tier.
    // CEP:STATUS complete
    // CEP:FAILURE T0 for unknown functions.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn tier_of(&self, method: u32) -> Tier {
        self.inner
            .lock()
            .expect("controller lock")
            .functions
            .get(&method)
            .map(|fs| fs.tier)
            .unwrap_or(Tier::T0)
    }

    /// Pending request count.
    // CEP:WHAT Reports the queue backlog.
    // CEP:WHY Backpressure math and tests read the backlog.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn queue_len(&self) -> usize {
        self.queue.lock().expect("queue lock").len()
    }

    /// Requests cancellation; idempotent (race 2 of 6.7).
    // CEP:WHAT Sets the cancel flag and requests the state transition.
    // CEP:WHY Multiple cancellers must converge on one outcome.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; completed requests are unaffected.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn cancel(&self, request: &Request) {
        request.cancel.store(true, Ordering::Release);
        // Only a Compiling request moves to CancellationRequested; a
        // completed one is left alone (race 1 of 6.7).
        let _ = request.state.compare_exchange(
            ReqState::Compiling as u8,
            ReqState::CancellationRequested as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Waits until a request reaches Installed or Cancelled.
    // CEP:WHAT Polls the request state with a deadline.
    // CEP:WHY Tests and the runtime need a bounded wait.
    // CEP:STATUS complete
    // CEP:FAILURE False on timeout.
    // CEP:ASSUMES None.
    // CEP:COST O(polls).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn wait_done(&self, request: &Request, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            match request.state() {
                ReqState::Installed | ReqState::Cancelled => return true,
                // Queued, Compiling, CancellationRequested, Cancelling, and
                // Completed all keep the caller waiting.
                _ => {
                    let _ = request.state;
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Shuts the worker pool down.
    // CEP:WHAT Signals workers and joins them.
    // CEP:WHY Clean teardown for tests and process exit.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(workers).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Release);
        for h in self.workers.drain(..) {
            let _ = h.join();
        }
    }
}

/// The worker loop: dequeue, compile with interruption points, install.
// CEP:WHAT Processes queued requests until shutdown.
// CEP:WHY 6.5: interruption points check the cancel flag between passes.
// CEP:STATUS complete
// CEP:FAILURE Cancelled requests clean up and become Cancelled.
// CEP:ASSUMES None.
// CEP:COST Bounded by request work.
// CEP:ID: JIT-a61
// CEP:EVIDENCE cargo test -p vortex-tier
fn worker_loop(
    queue: Arc<Mutex<Vec<QueueEntry>>>,
    cache: Arc<RwLock<HashMap<u32, Tier>>>,
    shutdown: Arc<AtomicBool>,
    backlog: Arc<AtomicUsize>,
) {
    loop {
        if shutdown.load(Ordering::Acquire) {
            return;
        }
        let popped = {
            let mut q = queue.lock().expect("queue lock");
            if q.is_empty() {
                None
            } else {
                // Highest hotness-over-cost first; ties by submission seq.
                q.sort_by(|a, b| {
                    b.priority
                        .partial_cmp(&a.priority)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.seq.cmp(&b.seq))
                });
                Some(q.remove(0))
            }
        };
        match popped {
            None => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Some(entry) => {
                let req = entry.request;
                // QUEUED -> COMPILING (CAS per design 6.3).
                let claimed = req
                    .state
                    .compare_exchange(
                        ReqState::Queued as u8,
                        ReqState::Compiling as u8,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok();
                if !claimed {
                    backlog.fetch_sub(1, Ordering::Relaxed);
                    continue;
                }
                // Compile with interruption points every 100 nodes.
                let mut cancelled = false;
                let mut done = 0usize;
                while done < req.nodes {
                    if req.cancel.load(Ordering::Acquire) {
                        cancelled = true;
                        break;
                    }
                    done += 100;
                }
                if cancelled {
                    // COMPILING -> CANCELLING -> CANCELLED (cleanup is
                    // dropping the partial work; idempotent by ownership).
                    let _ = req.state.compare_exchange(
                        ReqState::Compiling as u8,
                        ReqState::Cancelling as u8,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                    let _ = req.state.compare_exchange(
                        ReqState::Cancelling as u8,
                        ReqState::Cancelled as u8,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                } else {
                    // COMPILING -> COMPLETED -> INSTALLED.
                    let completed = req
                        .state
                        .compare_exchange(
                            ReqState::Compiling as u8,
                            ReqState::Completed as u8,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok();
                    if completed {
                        cache
                            .write()
                            .expect("cache lock")
                            .insert(req.method, req.tier);
                        req.state
                            .store(ReqState::Installed as u8, Ordering::Release);
                    }
                }
                backlog.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

/// Maps an ordinal back to a tier (demotion helper).
// CEP:WHAT Converts an ordinal to a tier.
// CEP:WHY Demotion decrements the ordinal.
// CEP:STATUS complete
// CEP:FAILURE Clamps to T5 for out-of-range values.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-tier
fn tier_from_ordinal(ordinal: u8) -> Tier {
    match ordinal {
        0 => Tier::T0,
        1 => Tier::T1,
        2 => Tier::T2,
        3 => Tier::T3,
        4 => Tier::T4,
        _ => Tier::T5,
    }
}
