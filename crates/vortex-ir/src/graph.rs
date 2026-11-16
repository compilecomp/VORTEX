// CEP:FILE crates/vortex-ir/src/graph.rs
// CEP:WHAT The ET-SoN graph: generational arena of nodes with typed edge
//        lists per edge kind, region-class state-token threading, and the
//        construction builder (design 6.3, 6.5, 6.7).
// CEP:WHY The sea of nodes is the central optimizing representation; its
//        arena layout follows the documented cost model.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Builder methods cannot produce out-of-range edges because
//        NodeId values are minted by the arena itself.
// CEP:ASSUMES Region classes are numbered 0..region_classes.
// CEP:COST O(1) node insertion amortized; O(edges) traversal.
// CEP:EVIDENCE cargo test -p vortex-ir
//! ET-SoN graph structure (design 6.3, 6.5, 6.7).

use crate::types::{EffectSet, FpSet, Type};

/// A node handle: index into the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

/// Node opcodes, partitioned into control, pure, and effectful classes.
/// ConstFloat carries an f64, so equality is PartialEq only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    // ---- Control (N_ctrl) ----
    /// Produces arguments and initial state tokens.
    Start,
    /// Control merge with k inputs.
    Region,
    /// Data merge at a Region.
    Phi,
    /// State merge for one region class at a Region.
    StatePhi,
    /// Two-way branch producing two control outputs.
    If,
    /// Terminates with return values and final state tokens.
    Return,
    /// Terminates without values.
    Stop,
    // ---- Pure (N_pure) ----
    /// Integer constant.
    ConstInt(i64),
    /// Floating-point constant.
    ConstFloat(f64),
    /// Integer addition.
    IAdd,
    /// Integer subtraction.
    ISub,
    /// Integer multiplication.
    IMul,
    /// Integer division.
    IDiv,
    /// Integer remainder.
    IRem,
    /// Bitwise and.
    IAnd,
    /// Bitwise or.
    IOr,
    /// Bitwise exclusive or.
    IXor,
    /// Shift left.
    IShl,
    /// Shift right.
    IShr,
    /// Boolean negation.
    Not,
    /// Equality comparison.
    Eq,
    /// Signed less-than comparison.
    Lt,
    /// Integer widening or narrowing cast.
    IntCast,
    /// Integer to float conversion.
    IntToFloat,
    /// Float to integer conversion.
    FloatToInt,
    /// Float addition.
    FAdd,
    /// Float subtraction.
    FSub,
    /// Float multiplication.
    FMul,
    /// Float division.
    FDiv,
    /// Identity pass-through used by simplification passes.
    Identity,
    /// Tuple field extraction.
    TupleExtract,
    /// Tuple field insertion.
    TupleInsert,
    /// Array element read (pure view of an unaliased array).
    ArrayGet,
    /// Array element write through an unaliased array.
    ArraySet,
    // ---- Effectful (N_eff) ----
    /// Memory read.
    Load,
    /// Memory write.
    Store,
    /// Atomic load with an ordering.
    AtomicLoad,
    /// Atomic store with an ordering.
    AtomicStore,
    /// Atomic read-modify-write with an operation and ordering.
    AtomicRmw,
    /// Compare-and-exchange with success and failure orderings.
    CompareExchange,
    /// Heap allocation.
    Allocate,
    /// Call with an exception handler.
    Invoke,
    /// Throw to the nearest handler.
    Throw,
    /// Unrecoverable abnormal termination.
    TrapNode,
    /// Deoptimization safepoint.
    Safepoint,
}

/// The node class implied by an opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeClass {
    /// Control nodes.
    Ctrl,
    /// Pure computation nodes.
    Pure,
    /// Effectful nodes.
    Eff,
}

impl Op {
    /// The class of this opcode.
    // CEP:WHAT Partitions opcodes into ctrl, pure, and effectful.
    // CEP:WHY The verifier checks structure per class.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn class(self) -> NodeClass {
        match self {
            Op::Start | Op::Region | Op::Phi | Op::StatePhi | Op::If | Op::Return | Op::Stop => {
                NodeClass::Ctrl
            }
            Op::Load
            | Op::Store
            | Op::AtomicLoad
            | Op::AtomicStore
            | Op::AtomicRmw
            | Op::CompareExchange
            | Op::Allocate
            | Op::Invoke
            | Op::Throw
            | Op::TrapNode
            | Op::Safepoint => NodeClass::Eff,
            _ => NodeClass::Pure,
        }
    }

    /// The exact effect set this opcode implies (invariant 6).
    // CEP:WHAT Returns the mandatory effect set of the opcode.
    // CEP:WHY Invariant 6: E(n) is exactly the opcode-implied set.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn effects(self) -> EffectSet {
        use crate::types::Effect as E;
        match self {
            Op::Load => EffectSet::of(E::MemRead),
            Op::Store => EffectSet::of(E::MemWrite),
            Op::AtomicLoad => EffectSet::of2(E::MemRead, E::Barrier),
            Op::AtomicStore => EffectSet::of2(E::MemWrite, E::Barrier),
            Op::AtomicRmw | Op::CompareExchange => {
                EffectSet::of3(E::MemRead, E::MemWrite, E::Barrier)
            }
            Op::Allocate => EffectSet::of(E::Alloc),
            Op::Invoke => EffectSet::of(E::Call),
            Op::Throw | Op::TrapNode => EffectSet::of(E::Trap),
            Op::Safepoint => EffectSet::of(E::Barrier),
            Op::StatePhi => EffectSet::of(E::MemRead),
            _ => EffectSet::PURE,
        }
    }

    /// Whether the node terminates a control path.
    // CEP:WHAT Reports whether the opcode is a control terminator.
    // CEP:WHY State chains end only at terminators (invariant 4).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn is_terminator(self) -> bool {
        matches!(self, Op::Return | Op::Stop | Op::Throw | Op::TrapNode)
    }

    /// Whether the opcode is a floating-point operation.
    // CEP:WHAT Reports FP classification for flag validation.
    // CEP:WHY Invariant 7 applies only to FP nodes.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn is_fp(self) -> bool {
        matches!(self, Op::FAdd | Op::FSub | Op::FMul | Op::FDiv)
    }
}

/// One arena node: opcode, type, edges per kind, effect and FP sets, and
/// the region-class footprint bitmask.
#[derive(Debug, Clone)]
pub struct Node {
    /// Opcode.
    pub op: Op,
    /// Result type.
    pub ty: Type,
    /// Data inputs.
    pub data: Vec<NodeId>,
    /// Control inputs.
    pub ctrl: Vec<NodeId>,
    /// State inputs: (region class, producer).
    pub state: Vec<(u32, NodeId)>,
    /// Exception edges: handler Regions (for Invoke, Throw).
    pub except: Vec<NodeId>,
    /// Effect set; validated to equal Op::effects for effectful nodes.
    pub effects: EffectSet,
    /// FP relaxation flags for FP nodes.
    pub fp: FpSet,
    /// Region-class footprint bitmask (bit r set = touches class r).
    pub footprint: u64,
}

/// The sea-of-nodes graph.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    nodes: Vec<Node>,
    /// Number of region classes (alias-analysis result).
    pub region_classes: u32,
}

impl Graph {
    /// An empty graph with `region_classes` classes.
    // CEP:WHAT Creates an empty graph.
    // CEP:WHY Builders start from an empty arena.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES region_classes <= 64 (footprint bitmask width).
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn new(region_classes: u32) -> Graph {
        Graph {
            nodes: Vec::new(),
            region_classes: region_classes.min(64),
        }
    }

    /// Node count.
    // CEP:WHAT Reports the arena size.
    // CEP:WHY Passes and tests iterate over the arena.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no nodes.
    // CEP:WHAT Reports emptiness.
    // CEP:WHY Clippy requires an explicit negation of len.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Node accessor.
    // CEP:WHAT Returns a node reference.
    // CEP:WHY Passes read nodes through this accessor.
    // CEP:STATUS complete
    // CEP:FAILURE None for dead or out-of-range ids.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0)
    }

    /// Mutable node accessor.
    // CEP:WHAT Returns a mutable node reference.
    // CEP:WHY Passes rewrite inputs through this accessor.
    // CEP:STATUS complete
    // CEP:FAILURE None for dead or out-of-range ids.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.0)
    }

    /// Iterates (id, node) pairs in arena order.
    // CEP:WHAT Enumerates all nodes.
    // CEP:WHY Verifier and passes sweep the arena.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) to create.
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().enumerate().map(|(i, n)| (NodeId(i), n))
    }

    /// Inserts a node and mints its id.
    // CEP:WHAT Adds a node to the arena.
    // CEP:WHY The builder is the single construction path.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Edges reference ids minted earlier.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn insert(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        id
    }

    /// All consumers (users) of a node across every edge kind.
    // CEP:WHAT Computes reverse adjacency of a node.
    // CEP:WHY State-chain termination checks need consumers.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(|N| * edges).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn consumers(&self, id: NodeId) -> Vec<(NodeId, EdgeSlot)> {
        let mut out = Vec::new();
        for (nid, n) in self.iter() {
            if n.data.contains(&id) {
                out.push((nid, EdgeSlot::Data));
            }
            if n.ctrl.contains(&id) {
                out.push((nid, EdgeSlot::Ctrl));
            }
            if n.state.iter().any(|(_, s)| *s == id) {
                out.push((nid, EdgeSlot::State));
            }
            if n.except.contains(&id) {
                out.push((nid, EdgeSlot::Except));
            }
        }
        out
    }

    /// Consumers of a node through state edges of one region class.
    // CEP:WHAT Computes class-r state consumers of a node id.
    // CEP:WHY The verifier threads StatePhi chains per class.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(|N|).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn consumers_of_class(&self, id: NodeId, r: u32) -> Vec<NodeId> {
        let mut out = Vec::new();
        for (nid, n) in self.iter() {
            if n.state.iter().any(|(sr, s)| *sr == r && *s == id) {
                out.push(nid);
            }
        }
        out
    }
}

/// Which edge list of a consumer references a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeSlot {
    /// Data input list.
    Data,
    /// Control input list.
    Ctrl,
    /// State input list.
    State,
    /// Exception edge list.
    Except,
}

/// Convenience builder threading state tokens per region class.
pub struct Builder {
    /// The graph under construction.
    pub graph: Graph,
    /// Current state-token producer per region class.
    state_top: Vec<Option<NodeId>>,
}

impl Builder {
    /// Starts a builder with `region_classes` region classes.
    // CEP:WHAT Initializes the builder's state-token tops.
    // CEP:WHY Construction threads one chain per region class.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(region_classes).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn new(region_classes: u32) -> Builder {
        Builder {
            graph: Graph::new(region_classes),
            state_top: vec![None; region_classes.min(64) as usize],
        }
    }

    /// Inserts a raw node.
    // CEP:WHAT Adds a fully specified node.
    // CEP:WHY Complex constructions bypass the convenience helpers.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn insert(&mut self, node: Node) -> NodeId {
        self.graph.insert(node)
    }

    /// Creates the Start node and seeds state tops.
    // CEP:WHAT Emits Start with initial state tokens.
    // CEP:WHY Every region class begins with Start's initial token.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(region_classes).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn start(&mut self, arg_types: Vec<Type>) -> NodeId {
        let rc = self.graph.region_classes;
        let id = self.graph.insert(Node {
            op: Op::Start,
            ty: Type::Tuple(arg_types.clone()),
            data: Vec::new(),
            ctrl: Vec::new(),
            state: Vec::new(),
            except: Vec::new(),
            effects: EffectSet::PURE,
            fp: FpSet::STRICT,
            footprint: 0,
        });
        for r in 0..rc {
            self.state_top[r as usize] = Some(id);
        }
        id
    }

    /// The current state producer of a region class.
    // CEP:WHAT Reads the state-chain top for a class.
    // CEP:WHY Effectful nodes thread from the current top.
    // CEP:STATUS complete
    // CEP:FAILURE None before Start.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn state_top(&self, r: u32) -> Option<NodeId> {
        self.state_top.get(r as usize).copied().flatten()
    }

    /// Inserts an effectful node touching `regions`, threading state tokens
    /// from the current tops.
    // CEP:WHAT Builds an effectful node with per-region state edges.
    // CEP:WHY Construction wires state-in per touched class (6.3).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; classes without a top get no state edge.
    // CEP:ASSUMES The node's opcode matches its effect set.
    // CEP:COST O(regions touched).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn effectful(
        &mut self,
        op: Op,
        ty: Type,
        data: Vec<NodeId>,
        regions: &[u32],
        footprint: u64,
    ) -> NodeId {
        let mut state = Vec::new();
        for r in regions {
            if let Some(top) = self.state_top(*r) {
                state.push((*r, top));
            }
        }
        let id = self.graph.insert(Node {
            op,
            ty,
            data,
            ctrl: Vec::new(),
            state,
            except: Vec::new(),
            effects: op.effects(),
            fp: FpSet::STRICT,
            footprint,
        });
        for r in regions {
            self.state_top[*r as usize] = Some(id);
        }
        id
    }

    /// Inserts a pure node.
    // CEP:WHAT Builds a pure computation node.
    // CEP:WHY Pure nodes carry no state edges.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn pure(&mut self, op: Op, ty: Type, data: Vec<NodeId>) -> NodeId {
        self.graph.insert(Node {
            op,
            ty,
            data,
            ctrl: Vec::new(),
            state: Vec::new(),
            except: Vec::new(),
            effects: EffectSet::PURE,
            fp: FpSet::STRICT,
            footprint: 0,
        })
    }

    /// Inserts an FP node with a flag set.
    // CEP:WHAT Builds a floating-point node with relaxation flags.
    // CEP:WHY FP nodes carry their flag set in the signature.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn fp_node(&mut self, op: Op, ty: Type, data: Vec<NodeId>, fp: FpSet) -> NodeId {
        self.graph.insert(Node {
            op,
            ty,
            data,
            ctrl: Vec::new(),
            state: Vec::new(),
            except: Vec::new(),
            effects: EffectSet::PURE,
            fp,
            footprint: 0,
        })
    }
}
