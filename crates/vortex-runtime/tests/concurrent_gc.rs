// CEP:FILE crates/vortex-runtime/tests/concurrent_gc.rs
// CEP:WHAT Tests for the concurrent GC shell: safepoint arbitration,
//        concurrent marking under mutator writes, card table, parking.
// CEP:WHY The concurrency protocol is only as good as these tests.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE A failing test blocks the CI gate.
// CEP:ASSUMES Deterministic quanta make schedules reproducible enough.
// CEP:COST O(tests).
// CEP:EVIDENCE cargo test -p vortex-runtime --test concurrent_gc
//! Concurrent GC protocol tests.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use vortex_runtime::concurrent_gc::{ConcurrentGc, Phase, SafepointState};

#[test]
fn arbitration_gc_beats_deopt() {
    let coord = vortex_runtime::concurrent_gc::SafepointCoordinator::new();
    // A deopt safepoint alone is grantable.
    assert!(coord.request_deopt());
    assert_eq!(coord.state(), SafepointState::DeoptPending);
    // A GC request displaces the pending deopt and defers it.
    coord.request_gc();
    assert_eq!(coord.state(), SafepointState::GcPending);
    assert_eq!(coord.deferred_deopts(), 1);
    // Mutators park for the GC safepoint.
    assert_eq!(
        coord.poll(),
        vortex_runtime::concurrent_gc::SafepointAction::ParkGc
    );
    coord.activate_gc();
    coord.finish_gc();
    assert_eq!(coord.state(), SafepointState::Running);
    // The deferred deopt can now be re-issued.
    assert!(coord.request_deopt());
    coord.run_deopt_window();
    assert_eq!(coord.state(), SafepointState::Running);
    // A deopt request during an active GC window is also deferred.
    coord.request_gc();
    coord.activate_gc();
    assert!(!coord.request_deopt());
    assert_eq!(coord.deferred_deopts(), 2);
    coord.finish_gc();
}

#[test]
fn concurrent_mark_survives_mutator_writes() {
    // Objects: 0 = garbage, 1 = root A, 2 = B, 3 = C (white, added by the
    // mutator during marking), old_base = 4 (all young here).
    let gc = ConcurrentGc::new(4, 4, 4096);
    gc.set_roots(&[1]);
    {
        let h = gc.handle();
        h.record_write(1, 2); // A -> B
    }
    gc.start_concurrent_mark();
    // Mutator thread stores A -> C while the worker is marking; SATB must
    // keep C alive through the remark.
    let h2 = gc.handle();
    let writer = std::thread::spawn(move || {
        for _ in 0..50 {
            h2.record_write(1, 3);
            std::thread::sleep(std::time::Duration::from_micros(50));
        }
    });
    // The worker finishes marking on its own; wait for the drain.
    gc.wait_phase(Phase::Idle);
    writer.join().expect("writer joins");
    gc.remark();
    let marked = gc.handle().mark_snapshot();
    assert!(marked[1], "root A must be marked");
    assert!(marked[2], "B reachable from A must be marked");
    assert!(marked[3], "C written during marking must survive SATB");
    assert!(!marked[0], "garbage must stay unmarked");
    gc.shutdown();
}

#[test]
fn card_table_tracks_old_to_young() {
    let gc = ConcurrentGc::new(8, 5, 4096);
    let h = gc.handle();
    // old(5) -> young(2) dirties a card; young(2) -> young(1) does not.
    h.record_write(5, 2);
    let cards = gc.dirty_cards();
    assert!(!cards.is_empty(), "old-to-young store must dirty a card");
    assert!(
        cards
            .iter()
            .all(|&c| c * vortex_runtime::concurrent_gc::CARD_SIZE <= 5),
        "dirtied card must cover the source object"
    );
    // After the drain the table is clean again.
    assert!(gc.dirty_cards().is_empty());
    h.record_write(2, 1);
    assert!(
        gc.dirty_cards().is_empty(),
        "young-to-young stores must not dirty cards"
    );
    gc.shutdown();
}

#[test]
fn evacuate_young_finds_old_roots_via_cards() {
    // old_base = 4: objects 0..4 young, 4..8 old.
    let gc = ConcurrentGc::new(8, 4, 4096);
    gc.set_roots(&[5]); // old root
    {
        let h = gc.handle();
        h.record_write(5, 2); // old -> young edge
        h.record_write(2, 1); // young -> young edge
    }
    gc.start_concurrent_mark();
    gc.wait_phase(Phase::Idle);
    gc.remark();
    // Dirty the card for the old->young edge (record_write did it, but
    // the young scan drains cards itself).
    gc.evacuate_young();
    let evacuated = gc.handle().evacuated();
    assert!(
        evacuated.contains(&2) && evacuated.contains(&1),
        "live young objects reachable from old roots must evacuate: {:?}",
        evacuated
    );
    assert!(
        !evacuated.contains(&0) && !evacuated.contains(&3),
        "dead young objects must not evacuate: {:?}",
        evacuated
    );
    gc.shutdown();
}

#[test]
fn mutators_park_during_gc_and_resume() {
    let gc = ConcurrentGc::new(4, 4, 4096);
    let parks = Arc::new(AtomicUsize::new(0));
    let mut joins = Vec::new();
    for _ in 0..2 {
        let h = gc.handle();
        let parks = Arc::clone(&parks);
        joins.push(std::thread::spawn(move || {
            for _ in 0..200 {
                match h.poll() {
                    vortex_runtime::concurrent_gc::SafepointAction::ParkGc => {
                        parks.fetch_add(1, Ordering::Relaxed);
                        h.park_while(SafepointState::GcActive);
                    }
                    _ => h.alloc(),
                }
                std::thread::sleep(std::time::Duration::from_micros(20));
            }
        }));
    }
    std::thread::sleep(std::time::Duration::from_millis(10));
    gc.coordinator().request_gc();
    gc.coordinator().activate_gc();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(
        gc.coordinator().parked() > 0,
        "mutators must park during GcActive"
    );
    gc.coordinator().finish_gc();
    for j in joins {
        j.join().expect("mutators join");
    }
    assert!(parks.load(Ordering::Relaxed) > 0);
    gc.shutdown();
}
