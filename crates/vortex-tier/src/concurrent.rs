// CEP:FILE crates/vortex-tier/src/concurrent.rs
// CEP:WHAT Concurrent coordination primitives: the safepoint gate that
//        parks mutator threads for STW phases, and the invalidation map
//        that publishes code-invalidation flags to all threads
//        (GC design 6.11; concurrent deopt design).
// CEP:WHY Stop-the-world phases and concurrent deoptimization require
//        every thread to observe the same flag state atomically.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Release always wakes parked threads; flags are never lost.
// CEP:ASSUMES Rust memory model: acquire/release orderings.
// CEP:COST O(1) per check; O(parked) on release.
// CEP:EVIDENCE cargo test -p vortex-tier
//! Safepoints and concurrent invalidation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

/// The safepoint gate (GC design 6.11): mutators park when the flag is
/// set and wake on release.
pub struct SafepointGate {
    requested: AtomicBool,
    parked_count: Mutex<usize>,
    cond: Condvar,
}

impl SafepointGate {
    /// A clear gate.
    // CEP:WHAT Creates the gate.
    // CEP:WHY STW phases coordinate through one flag.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn new() -> SafepointGate {
        SafepointGate {
            requested: AtomicBool::new(false),
            parked_count: Mutex::new(0),
            cond: Condvar::new(),
        }
    }

    /// Requests all mutators to park.
    // CEP:WHAT Sets the safepoint flag.
    // CEP:WHY The collector needs every mutator at a safepoint.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    /// Whether mutators must park.
    // CEP:WHAT Loads the flag.
    // CEP:WHY Mutators check at back-edges and returns.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn at_safepoint(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    /// Parks the calling thread until release or timeout.
    // CEP:WHAT Blocks on the condition variable.
    // CEP:WHY Parked mutators must not run during STW phases.
    // CEP:STATUS complete
    // CEP:FAILURE False on timeout.
    // CEP:ASSUMES None.
    // CEP:COST O(wait).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn park(&self, timeout: Duration) -> bool {
        if !self.at_safepoint() {
            return true;
        }
        {
            let mut c = self.parked_count.lock().expect("parked lock");
            *c += 1;
        }
        let deadline = Instant::now() + timeout;
        let mut c = self.parked_count.lock().expect("parked lock");
        let mut ok = true;
        while self.at_safepoint() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                ok = false;
                break;
            }
            let (guard, _) = self.cond.wait_timeout(c, remaining).expect("condvar lock");
            c = guard;
        }
        *c -= 1;
        ok
    }

    /// Releases all parked mutators.
    // CEP:WHAT Clears the flag and wakes every parked thread.
    // CEP:WHY Mutators resume after the STW phase.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn release(&self) {
        self.requested.store(false, Ordering::Release);
        self.cond.notify_all();
    }

    /// The number of currently parked threads.
    // CEP:WHAT Reports the parked count.
    // CEP:WHY Tests verify park and wake behavior.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn parked(&self) -> usize {
        *self.parked_count.lock().expect("parked lock")
    }
}

impl Default for SafepointGate {
    // CEP:WHAT Provides the default gate.
    // CEP:WHY One gate coordinates all phases.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    fn default() -> SafepointGate {
        SafepointGate::new()
    }
}

/// A shared invalidation flag for one compiled method (concurrent deopt:
/// "the invalidation is visible to all threads").
pub struct Invalidation {
    flag: Arc<AtomicBool>,
}

impl Invalidation {
    /// Registers one method's flag.
    // CEP:WHAT Creates a clear invalidation flag.
    // CEP:WHY Threads snapshot the Arc and poll it at safepoints.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn new() -> Invalidation {
        Invalidation {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Marks the method's code invalid.
    // CEP:WHAT Sets the flag; idempotent.
    // CEP:WHY Invalidation is atomic and visible to every holder.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn invalidate(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// Whether the code was invalidated.
    // CEP:WHAT Loads the flag.
    // CEP:WHY Threads check before entering compiled code.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn is_invalidated(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// A shareable snapshot for other threads.
    // CEP:WHAT Clones the flag handle.
    // CEP:WHY Arc sharing is the publish mechanism.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn share(&self) -> Arc<AtomicBool> {
        self.flag.clone()
    }
}

impl Default for Invalidation {
    // CEP:WHAT Provides the default flag.
    // CEP:WHY One flag per method.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    fn default() -> Invalidation {
        Invalidation::new()
    }
}

/// The registry of invalidations by method.
#[derive(Default)]
pub struct InvalidationMap {
    map: RwLock<HashMap<u32, Invalidation>>,
}

impl InvalidationMap {
    /// Invalidates a method, registering it on first use.
    // CEP:WHAT Marks a method's code invalid.
    // CEP:WHY Concurrent deopt publishes invalidation through this map.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn invalidate(&self, method: u32) {
        let mut m = self.map.write().expect("map lock");
        let flag = m.entry(method).or_default();
        flag.invalidate();
    }

    /// Whether a method's code is invalid.
    // CEP:WHAT Reads a method's flag.
    // CEP:WHY Threads check before entering compiled code.
    // CEP:STATUS complete
    // CEP:FAILURE False for never-compiled methods.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn is_invalidated(&self, method: u32) -> bool {
        self.map
            .read()
            .expect("map lock")
            .get(&method)
            .map(|f| f.is_invalidated())
            .unwrap_or(false)
    }

    /// A shareable flag for one method.
    // CEP:WHAT Returns the method's flag handle.
    // CEP:WHY Long-running threads poll a snapshot instead of the map.
    // CEP:STATUS complete
    // CEP:FAILURE Registers the method if absent.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn snapshot(&self, method: u32) -> Arc<AtomicBool> {
        let mut m = self.map.write().expect("map lock");
        let flag = m.entry(method).or_default();
        flag.share()
    }
}
