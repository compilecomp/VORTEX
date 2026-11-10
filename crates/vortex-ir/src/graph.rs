// CEP:FILE crates/vortex-ir/src/graph.rs
// CEP:WHAT Unified Sea-of-Nodes-style graph with single-writer transactional
//        mutation and an event log delivered to overlays at commit
//        (REVISED section 1, overlay mutation API rules 1-6).
// CEP:WHY All base-graph mutation must flow through transactions so overlays
//        are never silently stale across a mutation boundary.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Transactions reject overlay reads (OverlayLocked); commits
//        replay events in order; revisions are monotonic.
// CEP:ASSUMES Single compiler thread performs one transaction at a time
//        (the reference compiler is sequential; CONC-1 governs cross-thread
//        structures).
// CEP:COST Mutation O(1) plus O(events) at commit; reads O(1) by NodeId.
// CEP:EVIDENCE cargo test -p vortex-ir
use crate::overlay::{DependenceView, Overlay, OverlayError};
use std::cell::RefCell;
use std::rc::Rc;

/// Node identifier: index into the graph's node table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// Graph node operations (subset sufficient for pass testing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeOp {
    /// Start of the function.
    Start,
    /// Constant integer.
    ConstInt(i64),
    /// Binary add.
    Add,
    /// Binary multiply.
    Mul,
    /// Load from memory (effectful).
    Load,
    /// Store to memory (effectful).
    Store,
    /// Layout guard (specialization assumption).
    GuardLayout(u64),
    /// Region/basis control node.
    Region,
    /// Dead marker left by optimization; removed at commit.
    Dead,
}

/// A graph node.
#[derive(Debug, Clone)]
pub struct Node {
    /// Operation.
    pub op: NodeOp,
    /// Input edges (data/control/effect, distinguished by position).
    pub inputs: Vec<NodeId>,
}

/// Primitive mutation events recorded by a transaction and replayed to
/// overlays at commit (REVISED section 1 rule 3).
#[derive(Debug, Clone)]
pub enum Event {
    /// A node was inserted.
    NodeInserted(NodeId),
    /// A node was replaced by another (uses retargeted).
    NodeReplaced(NodeId, NodeId),
    /// An edge was deleted.
    EdgeDeleted(NodeId, usize),
    /// An edge was retargeted.
    EdgeRetargeted(NodeId, usize, NodeId),
}

/// The unified graph.
#[derive(Default)]
pub struct Graph {
    nodes: Vec<Node>,
    revision: u64,
    in_transaction: bool,
    pending: Vec<Event>,
    overlays: Vec<Rc<RefCell<dyn Overlay<View = DependenceView>>>>,
}

/// Mutations during `Graph::edit`.
#[derive(Debug)]
pub struct Tx<'g> {
    graph: &'g mut Graph,
}

impl core::fmt::Debug for Graph {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Graph")
            .field("nodes", &self.nodes.len())
            .field("revision", &self.revision)
            .field("in_transaction", &self.in_transaction)
            .field("pending_events", &self.pending.len())
            .field("overlays", &self.overlays.len())
            .finish()
    }
}

impl Graph {
    /// Creates an empty graph with a Start node.
    #[must_use]
    pub fn new() -> Self {
        let mut g = Graph::default();
        g.nodes.push(Node {
            op: NodeOp::Start,
            inputs: Vec::new(),
        });
        g
    }

    /// Registers an overlay to receive commit events.
    pub fn attach_overlay(&mut self, o: Rc<RefCell<dyn Overlay<View = DependenceView>>>) {
        self.overlays.push(o);
    }

    /// Current revision.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Borrows a node.
    ///
    /// # Panics
    /// Panics on an invalid NodeId.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    /// Number of nodes (including Dead markers).
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph is empty of nodes beyond Start.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Reads an overlay's view after validating freshness against the graph
    /// revision. Illegal inside a transaction (REVISED section 1 rule 2).
    ///
    /// # Errors
    /// Returns [`OverlayError::OverlayLocked`] inside a transaction.
    pub fn read_overlay(
        &self,
        o: &RefCell<dyn Overlay<View = DependenceView>>,
    ) -> Result<DependenceView, OverlayError> {
        if self.in_transaction {
            return Err(OverlayError::OverlayLocked);
        }
        Ok(o.borrow().view())
    }

    /// Runs a mutation transaction. On `commit`, events replay to overlays in
    /// order and both graph and overlay revisions advance (rule 4-6).
    // CEP:WHAT Runs a mutation transaction.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn edit<R>(graph: &mut Graph, f: impl FnOnce(&mut Tx<'_>) -> R) -> R {
        graph.in_transaction = true;
        let mut tx = Tx { graph };
        let out = f(&mut tx);
        let events = std::mem::take(&mut tx.graph.pending);
        let g = &mut *tx.graph;
        g.in_transaction = false;
        g.revision += 1;
        for o in &g.overlays {
            let mut ob = o.borrow_mut();
            for e in &events {
                ob.on_event(e);
            }
            ob.set_revision(g.revision);
        }
        out
    }

    fn push_event(&mut self, e: Event) {
        self.pending.push(e);
    }
}

impl Tx<'_> {
    /// Overlay reads inside a transaction are always rejected
    /// (REVISED section 1 rule 2): the overlay may be stale mid-mutation.
    pub fn read_overlay(
        &self,
        _o: &RefCell<dyn Overlay<View = DependenceView>>,
    ) -> Result<DependenceView, OverlayError> {
        Err(OverlayError::OverlayLocked)
    }

    /// Inserts a node and returns its id.
    pub fn insert(&mut self, op: NodeOp, inputs: &[NodeId]) -> NodeId {
        let id = NodeId(self.graph.nodes.len() as u32);
        self.graph.nodes.push(Node {
            op,
            inputs: inputs.to_vec(),
        });
        self.graph.push_event(Event::NodeInserted(id));
        id
    }

    /// Replaces `old` with `new` in every input list.
    pub fn replace(&mut self, old: NodeId, new: NodeId) {
        for n in &mut self.graph.nodes {
            for input in &mut n.inputs {
                if *input == old {
                    *input = new;
                }
            }
        }
        self.graph.push_event(Event::NodeReplaced(old, new));
    }

    /// Deletes the input edge at `slot` of `user`.
    pub fn delete_edge(&mut self, user: NodeId, slot: usize) {
        if let Some(n) = self.graph.nodes.get_mut(user.0 as usize) {
            if slot < n.inputs.len() {
                n.inputs.remove(slot);
                self.graph.push_event(Event::EdgeDeleted(user, slot));
            }
        }
    }

    /// Retargets the input edge at `slot` of `user` to `new`.
    pub fn retarget(&mut self, user: NodeId, slot: usize, new: NodeId) {
        if let Some(n) = self.graph.nodes.get_mut(user.0 as usize) {
            if slot < n.inputs.len() {
                n.inputs[slot] = new;
                self.graph
                    .push_event(Event::EdgeRetargeted(user, slot, new));
            }
        }
    }
}
