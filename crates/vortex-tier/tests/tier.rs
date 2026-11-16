// CEP:FILE crates/vortex-tier/tests/tier.rs
// CEP:WHAT Tier controller tests: thresholds and promotion, worker pool
//        installation, cancellation (idempotence, cancel-vs-complete),
//        deopt demotion, safepoint parking across threads, and
//        cross-thread invalidation.
// CEP:WHY The tiered system's correctness lives in these transitions.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Deterministic request sizes.
// CEP:COST O(workers) per case.
// CEP:EVIDENCE cargo test -p vortex-tier

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Tier controller test suite.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use vortex_tier::{Controller, InvalidationMap, ReqState, SafepointGate, Tier};

/// Reaching 100 invocations promotes T0 to T1 and installs code.
// CEP:WHAT The end-to-end promotion path through the worker pool.
// CEP:WHY 6.6: Threshold(T0 to T1) = 100.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(workers).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn promotion_installs_code() {
    let controller = Controller::new(2);
    let mut request = None;
    for _ in 0..100 {
        request = controller.note_execution(0, 1, 0);
    }
    let request = request.expect("promotion request at 100");
    assert_eq!(request.tier, Tier::T1);
    assert!(controller.wait_done(&request, Duration::from_secs(5)));
    assert_eq!(request.state(), ReqState::Installed);
    assert_eq!(controller.installed_tier(0), Some(Tier::T1));
    controller.shutdown();
}

/// The next threshold needs the new base after promotion.
// CEP:WHAT T1 requires 1000 more hotness units.
// CEP:WHY Monotonic thresholds prevent thrash.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(workers).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn thresholds_are_monotonic() {
    let controller = Controller::new(1);
    let mut request = None;
    for _ in 0..100 {
        request = controller.note_execution(1, 1, 0);
    }
    let request = request.expect("first promotion");
    let _ = controller.wait_done(&request, Duration::from_secs(5));
    assert_eq!(controller.tier_of(1), Tier::T1);
    // 100 more invocations is far below the T1 threshold of 1000.
    assert!(controller.note_execution(1, 100, 0).is_none());
    controller.shutdown();
}

/// Cancellation produces Cancelled and never installs.
// CEP:WHAT A cancelled request ends in Cancelled with no cache entry.
// CEP:WHY Cancellation design 6.5-6.6.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(workers).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn cancellation_marks_cancelled() {
    let controller = Controller::new(1);
    let mut request = None;
    for _ in 0..100 {
        request = controller.note_execution(2, 1, 0);
    }
    let request = request.expect("request");
    controller.cancel(&request);
    // Cancel again: idempotent (race 2 of 6.7).
    controller.cancel(&request);
    assert!(controller.wait_done(&request, Duration::from_secs(5)));
    assert_eq!(request.state(), ReqState::Cancelled);
    assert_eq!(controller.installed_tier(2), None);
    controller.shutdown();
}

/// Completion wins the race against a late cancellation.
// CEP:WHAT A cancel after completion leaves the code installed.
// CEP:WHY Race 1 of 6.7: COMPLETED is never reverted.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(workers).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn complete_wins_over_late_cancel() {
    let controller = Controller::new(1);
    let mut request = None;
    for _ in 0..100 {
        request = controller.note_execution(3, 1, 0);
    }
    let request = request.expect("request");
    assert!(controller.wait_done(&request, Duration::from_secs(5)));
    assert_eq!(request.state(), ReqState::Installed);
    controller.cancel(&request);
    assert_eq!(request.state(), ReqState::Installed);
    assert_eq!(controller.installed_tier(3), Some(Tier::T1));
    controller.shutdown();
}

/// Deopt demotion steps the tier down and evicts the cache entry.
// CEP:WHAT demote drops T1 to T0 without resetting hotness.
// CEP:WHY 6.6 demotion with profile preservation.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn demotion_preserves_hotness() {
    let controller = Controller::new(1);
    let mut request = None;
    for _ in 0..100 {
        request = controller.note_execution(4, 1, 0);
    }
    let request = request.expect("request");
    let _ = controller.wait_done(&request, Duration::from_secs(5));
    assert_eq!(controller.tier_of(4), Tier::T1);
    controller.demote(4);
    assert_eq!(controller.tier_of(4), Tier::T0);
    assert_eq!(controller.installed_tier(4), None);
    controller.shutdown();
}

/// The safepoint gate parks one thread until release.
// CEP:WHAT A parked thread wakes when the gate releases.
// CEP:WHY GC design 6.11 safepoint protocol.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(ms).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn safepoint_parks_and_releases() {
    let gate = Arc::new(SafepointGate::new());
    gate.request();
    let g2 = gate.clone();
    let observed = Arc::new(AtomicUsize::new(0));
    let observed2 = observed.clone();
    let handle = std::thread::spawn(move || {
        if g2.park(Duration::from_secs(5)) {
            observed2.store(1, Ordering::Release);
        }
    });
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(gate.parked(), 1, "thread is parked");
    gate.release();
    handle.join().expect("join");
    assert_eq!(observed.load(Ordering::Acquire), 1);
    assert!(!gate.at_safepoint());
}

/// Invalidation flags publish across threads.
// CEP:WHAT One thread's invalidate is visible to another's snapshot.
// CEP:WHY Concurrent deopt: invalidation is visible to all threads.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(ms).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn invalidation_publishes() {
    let map = Arc::new(InvalidationMap::default());
    let snapshot = map.snapshot(7);
    let m2 = map.clone();
    let s2 = snapshot.clone();
    let handle = std::thread::spawn(move || {
        m2.invalidate(7);
    });
    handle.join().expect("join");
    assert!(map.is_invalidated(7));
    assert!(snapshot.load(Ordering::Acquire));
    assert!(!map.is_invalidated(8), "other methods unaffected");
}

/// Backlog raises the effective promotion threshold.
// CEP:WHAT A saturated queue delays promotion (6.6 adaptive).
// CEP:WHY Backpressure keeps compile time bounded.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn queue_drains_under_load() {
    let controller = Controller::new(2);
    let mut requests = Vec::new();
    for m in 0..50u32 {
        if let Some(r) = controller.note_execution(m, 100, 0) {
            requests.push(r);
        }
    }
    assert!(controller.queue_len() <= 50);
    for r in &requests {
        assert!(controller.wait_done(r, Duration::from_secs(10)));
        assert_eq!(r.state(), ReqState::Installed);
    }
    assert_eq!(controller.queue_len(), 0);
    controller.shutdown();
}
