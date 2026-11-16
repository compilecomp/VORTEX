// CEP:FILE crates/vortex-ir/src/lib.rs
// CEP:WHAT Crate root for the ET-SoN IR: types and lattices, the arena
//        graph, the verifier, GVN, and deoptimization metadata.
// CEP:WHY ET-SoN is the central target-independent representation; every
//        optimizing tier consumes it through this crate.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-ir
//! ET-SoN: Effect-Typed Sea of Nodes IR.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod deopt;
pub mod graph;
pub mod gvn;
pub mod types;
pub mod verify;

pub use deopt::{DeoptMeta, DeoptValue};
pub use graph::{Builder, EdgeSlot, Graph, Node, NodeClass, NodeId, Op};
pub use gvn::gvn;
pub use types::{Effect, EffectSet, FpFlag, FpSet, Type};
pub use verify::{verify, VerifyError};
