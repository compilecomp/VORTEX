//! Unified optimizing graph IR with VSDG dependence overlays and the
//! FFI SemanticCompatibilityMatrix.
// CEP:FILE crates/vortex-ir/src/lib.rs
// CEP:WHAT Crate root: re-exports for the graph, overlay, GVN pass, and
//        compatibility matrix.
// CEP:WHY The unified graph is the single canonical optimizing IR (REVISED
//        section 1); overlays and the FFI matrix are its two reviewed
//        contracts.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-ir

/// FFI SemanticCompatibilityMatrix (FFI-1 5).
pub mod compat;
/// Unified graph with transactional mutation and event log.
pub mod graph;
/// GVN pass operating through transactions.
pub mod gvn;
/// VSDG dependence overlay (REVISED 1).
pub mod overlay;

pub use compat::{check_matrix, Domain, ExnClass, InlinePlan, LangProfile, RejectReason};
pub use graph::{Event, Graph, Node, NodeId, NodeOp, Tx};
pub use gvn::gvn;
pub use overlay::{DependenceOverlay, Overlay, OverlayError};
