// CEP:FILE crates/vortex-ir/tests/ir.rs
// CEP:WHAT IR tests: the overlay transaction API (REVISED section 1 rules
//        1-6) and the SemanticCompatibilityMatrix (FFI-1 section 5).
// CEP:WHY These two contracts were the review's "needs definition" items;
//        the tests are their executable definitions.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes) per case.
// CEP:EVIDENCE cargo test -p vortex-ir

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! IR overlay/matrix test suite.

use std::cell::RefCell;
use std::rc::Rc;

use vortex_ir::{
    check_matrix, gvn, DependenceOverlay, Domain, Event, ExnClass, Graph, LangProfile, NodeId,
    NodeOp, Overlay,
};

/// GVN deduplicates pure nodes transactionally: the add and the constant are
/// replaced by their leaders; effectful nodes are untouched.
// CEP:WHAT GVN deduplicates pure nodes transactionally: the add and the constant are replaced by their leaders; effectful nodes are untouched.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn gvn_deduplicates_pure_nodes() {
    let mut g = Graph::new();
    let start = NodeId(0);
    let (mut c1, mut c2, mut a1, mut a2, mut load, mut store) = (
        NodeId(0),
        NodeId(0),
        NodeId(0),
        NodeId(0),
        NodeId(0),
        NodeId(0),
    );
    Graph::edit(&mut g, |tx| {
        c1 = tx.insert(NodeOp::ConstInt(7), &[]);
        c2 = tx.insert(NodeOp::ConstInt(7), &[]);
        a1 = tx.insert(NodeOp::Add, &[c1, c2]);
        a2 = tx.insert(NodeOp::Add, &[c2, c1]);
        load = tx.insert(NodeOp::Load, &[start]);
        store = tx.insert(NodeOp::Store, &[load, a1]);
        let _ = store;
    });
    let _ = (c1, c2, a1, a2, load, store);
    let replaced = gvn(&mut g);
    assert_eq!(replaced, 2, "c2 and a2 are redundant");
}

/// Overlay API rule 2: overlay reads inside a transaction are rejected.
// CEP:WHAT Overlay API rule 2: overlay reads inside a transaction are rejected.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn overlay_reads_are_locked_during_transactions() {
    let mut g = Graph::new();
    let o = Rc::new(RefCell::new(DependenceOverlay::new()));
    g.attach_overlay(o.clone());

    Graph::edit(&mut g, |tx| {
        let err = tx
            .read_overlay(o.as_ref())
            .expect_err("locked during transaction");
        assert_eq!(err, vortex_ir::OverlayError::OverlayLocked);
    });
    // Outside the transaction the read succeeds.
    assert!(g.read_overlay(o.as_ref()).is_ok());
}

/// Overlay API rules 3-6: events replay to the overlay at commit in order,
/// the overlay revision matches the graph revision, and an edge deletion
/// marks the overlay stale (the base graph is the truth; the overlay is a
/// cache).
// CEP:WHAT Overlay API rules 3-6: events replay to the overlay at commit in order, the overlay revision matches the graph revision, and an edge deletion marks the overlay stale (the base graph is the truth; the overlay is a cache).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn overlay_receives_events_and_versions_track() {
    let mut g = Graph::new();
    let o = Rc::new(RefCell::new(DependenceOverlay::new()));
    g.attach_overlay(o.clone());
    assert_eq!(o.borrow().revision(), 0);
    assert_eq!(g.revision(), 0);

    let (mut a, mut b, mut user) = (NodeId(0), NodeId(0), NodeId(0));
    Graph::edit(&mut g, |tx| {
        a = tx.insert(NodeOp::ConstInt(1), &[]);
        b = tx.insert(NodeOp::ConstInt(2), &[]);
        user = tx.insert(NodeOp::Add, &[a, b]);
        tx.delete_edge(user, 1);
    });
    let _ = (a, b, user);

    assert_eq!(g.revision(), 1, "one transaction, one revision");
    assert_eq!(o.borrow().revision(), 1, "overlay synced at commit");
    let view = g
        .read_overlay(o.as_ref())
        .expect("readable outside transaction");
    assert!(
        view.rebuilt,
        "edge deletion degrades facts: overlay reports stale"
    );
    // The overlay saw NodeInserted x3 and EdgeDeleted x1.
    assert_eq!(view.facts.len(), 3);
}

/// The overlay is rebuilt by reseeding after staleness (rule 4: overlay is a
/// cache; base graph is the truth).
// CEP:WHAT The overlay is rebuilt by reseeding after staleness (rule 4: overlay is a cache; base graph is the truth).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn stale_overlay_can_be_reseeded() {
    let mut o = DependenceOverlay::new();
    o.on_event(&Event::NodeInserted(NodeId(1)));
    assert!(!o.is_stale());
    o.on_event(&Event::EdgeDeleted(NodeId(1), 0));
    assert!(
        o.is_stale(),
        "effect-edge deletion invalidates cached purity"
    );
    o.seed(std::iter::empty());
    assert!(!o.is_stale(), "rebuild clears staleness");
}

/// FFI-1 section 5: a fully compatible boundary inlines; every dimension's
/// failure is reported with its specific reason.
// CEP:WHAT FFI-1 section 5: a fully compatible boundary inlines; every dimension's failure is reported with its specific reason.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn compatibility_matrix_gates_inlining() {
    // Compatible: pure integer-domain callee, caller handles its raises.
    let caller = LangProfile {
        can_raise: vec![],
        handlers: vec![ExnClass::Runtime, ExnClass::Numeric],
        orders_globals: false,
        domain: Domain::Int48,
        continuation_half: false,
    };
    let callee = LangProfile {
        can_raise: vec![ExnClass::Numeric],
        handlers: vec![],
        orders_globals: false,
        domain: Domain::Int48,
        continuation_half: false,
    };
    let plan = check_matrix(&caller, &callee).expect("compatible boundary");
    assert!(
        !plan.exception_adapter_elided,
        "callee raises: adapter stays"
    );

    // Exception visibility failure.
    let callee_bad_exn = LangProfile {
        can_raise: vec![ExnClass::Foreign],
        ..callee.clone()
    };
    assert_eq!(
        check_matrix(&caller, &callee_bad_exn),
        Err(vortex_ir::RejectReason::ExceptionVisibility)
    );

    // Ordering failure.
    let callee_orders = LangProfile {
        orders_globals: true,
        ..callee.clone()
    };
    assert_eq!(
        check_matrix(&caller, &callee_orders),
        Err(vortex_ir::RejectReason::Ordering)
    );

    // Value-semantics failure.
    let callee_f64 = LangProfile {
        domain: Domain::Float64,
        ..callee.clone()
    };
    assert_eq!(
        check_matrix(&caller, &callee_f64),
        Err(vortex_ir::RejectReason::ValueSemantics)
    );

    // Continuation failure.
    let callee_continuation = LangProfile {
        continuation_half: true,
        ..callee.clone()
    };
    assert_eq!(
        check_matrix(&caller, &callee_continuation),
        Err(vortex_ir::RejectReason::Continuation)
    );

    // Unknown domains are conservative rejections, never permissive.
    let callee_unknown = LangProfile {
        domain: Domain::Unknown,
        ..callee
    };
    assert_eq!(
        check_matrix(&caller, &callee_unknown),
        Err(vortex_ir::RejectReason::ValueSemantics)
    );
}

/// A fully-adapter-free boundary elides the exception adapter.
// CEP:WHAT A fully-adapter-free boundary elides the exception adapter.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn fully_compatible_boundary_elides_adapters() {
    let caller = LangProfile {
        can_raise: vec![],
        handlers: vec![],
        orders_globals: false,
        domain: Domain::Int48,
        continuation_half: false,
    };
    let callee = LangProfile {
        can_raise: vec![],
        handlers: vec![],
        orders_globals: false,
        domain: Domain::Int48,
        continuation_half: false,
    };
    let plan = check_matrix(&caller, &callee).expect("pure boundary");
    assert!(plan.exception_adapter_elided);
    assert!(!plan.requires_safepoint_rewrite);
}
