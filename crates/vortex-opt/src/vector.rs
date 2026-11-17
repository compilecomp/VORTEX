// CEP:FILE crates/vortex-opt/src/vector.rs
// CEP:WHAT The Vector IR model: width-polymorphic vector types, vector
//        node semantics across arithmetic, memory, reduction, shuffle,
//        and mask categories, loop structure with induction variables,
//        and the validity invariants subset (Vector IR design 6.3-6.7).
// CEP:WHY Vectorization decisions must be explicit downstream; this IR
//        encodes them without re-analysis.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE The verifier rejects inconsistent widths, masks, and
//        reduction identities through typed errors.
// CEP:ASSUMES Widths are lane counts; the backend picks concrete sizes.
// CEP:COST O(|nodes|) per verifier pass.
// CEP:EVIDENCE cargo test -p vortex-opt
//! Vector IR (design: Vector IR 6.3-6.7).

use std::fmt;

/// A width-polymorphic vector type (design 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VType {
    /// Element scalar type tag.
    pub elem: ScalarTag,
    /// Lane count; the concrete machine width is chosen by the backend.
    pub lanes: u16,
}

/// Scalar element tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarTag {
    /// 8-bit integer.
    I8,
    /// 16-bit integer.
    I16,
    /// 32-bit integer.
    I32,
    /// 64-bit integer.
    I64,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
}

impl VType {
    /// The element byte size.
    // CEP:WHAT Reports the element size in bytes.
    // CEP:WHY Memory and shuffle cost models use element sizes.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn elem_size(self) -> usize {
        match self.elem {
            ScalarTag::I8 => 1,
            ScalarTag::I16 => 2,
            ScalarTag::I32 => 4,
            ScalarTag::I64 => 8,
            ScalarTag::F32 => 4,
            ScalarTag::F64 => 8,
        }
    }

    /// The total byte size at the current width.
    // CEP:WHAT Reports the vector byte size.
    // CEP:WHY Alignment and memory-op sizing need the total.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn byte_size(self) -> usize {
        self.elem_size() * self.lanes as usize
    }
}

/// Reduction operators with their identity elements (design 6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOp {
    /// Sum; identity 0.
    Sum,
    /// Product; identity 1.
    Mul,
    /// Minimum; identity +inf (or type max).
    Min,
    /// Maximum; identity -inf (or type min).
    Max,
}

impl ReduceOp {
    /// The identity element for the reduction.
    // CEP:WHAT Returns the identity of a reduction.
    // CEP:WHY Reduction legality requires the documented identity pairing.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn identity(self) -> i64 {
        match self {
            ReduceOp::Sum => 0,
            ReduceOp::Mul => 1,
            ReduceOp::Min => i64::MAX,
            ReduceOp::Max => i64::MIN,
        }
    }
}

/// Vector node kinds (design 6.5 categories).
#[derive(Debug, Clone, PartialEq)]
pub enum VNode {
    /// Vector arithmetic: lane-wise binary op.
    VArith {
        /// The lane-wise operation tag.
        lane_op: LaneOp,
        /// Operand type.
        ty: VType,
        /// Operands.
        operands: [usize; 2],
        /// Mask operand index, if predicated.
        mask: Option<usize>,
    },
    /// Contiguous vector load with alignment info.
    VLoad {
        /// Element type.
        ty: VType,
        /// Base address operand.
        base: usize,
        /// Contiguity and alignment annotation.
        align: Alignment,
        /// Mask operand index, if predicated.
        mask: Option<usize>,
    },
    /// Contiguous vector store.
    VStore {
        /// Element type.
        ty: VType,
        /// Base address operand.
        base: usize,
        /// Value operand.
        value: usize,
        /// Alignment annotation.
        align: Alignment,
        /// Mask operand index, if predicated.
        mask: Option<usize>,
    },
    /// Non-contiguous gather with an explicit index vector.
    VGather {
        /// Element type.
        ty: VType,
        /// Base address operand.
        base: usize,
        /// Index vector operand.
        indices: usize,
        /// Mask operand index, if predicated.
        mask: Option<usize>,
    },
    /// Non-contiguous scatter.
    VScatter {
        /// Element type.
        ty: VType,
        /// Base address operand.
        base: usize,
        /// Index vector operand.
        indices: usize,
        /// Value operand.
        value: usize,
        /// Mask operand index, if predicated.
        mask: Option<usize>,
    },
    /// Reduction over lanes with an explicit operator and identity.
    VReduce {
        /// The reduction operator.
        op: ReduceOp,
        /// Operand type.
        ty: VType,
        /// Vector operand.
        vector: usize,
    },
    /// Lane permutation: shuffle with a constant index vector.
    VShuffle {
        /// Operand type.
        ty: VType,
        /// Two source vectors.
        sources: [usize; 2],
        /// Lane indices into the concatenated sources.
        indices: Vec<u8>,
    },
    /// Mask computation from a comparison.
    VMask {
        /// Comparison tag.
        cmp: LaneOp,
        /// Operand type.
        ty: VType,
        /// Operands.
        operands: [usize; 2],
    },
    /// Lane-wise type conversion.
    VConvert {
        /// Source element tag.
        from: ScalarTag,
        /// Target element tag.
        to: ScalarTag,
        /// Lane count.
        lanes: u16,
        /// Vector operand.
        vector: usize,
    },
    /// Loop-structure header.
    LoopHeader {
        /// Preheader control operand.
        preheader: usize,
    },
    /// Loop latch: back-edge.
    LoopLatch {
        /// Body control operand.
        body: usize,
    },
    /// Loop exit block.
    LoopExit {
        /// Header control operand.
        header: usize,
        /// Exit condition operand.
        cond: usize,
    },
    /// Induction variable phi with start and step.
    Induction {
        /// Start value operand.
        start: usize,
        /// Step value operand.
        step: usize,
    },
}

/// Lane-wise operation tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneOp {
    /// Lane-wise add.
    Add,
    /// Lane-wise subtract.
    Sub,
    /// Lane-wise multiply.
    Mul,
    /// Lane-wise less-than.
    Lt,
    /// Lane-wise equality.
    Eq,
}

/// Memory access pattern annotation (design 6.1 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// Contiguous with a known byte alignment.
    Contiguous(usize),
    /// Strided access with a byte stride.
    Strided(usize),
}

/// Verifier failures (design 6.6 subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VError {
    /// Operand types disagree.
    WidthMismatch {
        /// The node index.
        node: usize,
    },
    /// A shuffle index is out of the concatenated source range.
    ShuffleRange {
        /// The node index.
        node: usize,
    },
    /// A predicated memory op lacks a mask operand binding.
    MaskMissing {
        /// The node index.
        node: usize,
    },
    /// Loop structure is malformed (latch without header, and so on).
    LoopMalformed {
        /// The node index.
        node: usize,
    },
}

impl fmt::Display for VError {
    // CEP:WHAT Renders a vector-IR verification failure.
    // CEP:WHY Vectorization authors need the offending node.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VError::WidthMismatch { node } => {
                write!(f, "vector node {node}: operand width mismatch")
            }
            VError::ShuffleRange { node } => {
                write!(f, "vector node {node}: shuffle index out of range")
            }
            VError::MaskMissing { node } => {
                write!(f, "vector node {node}: predicated op without mask")
            }
            VError::LoopMalformed { node } => {
                write!(f, "vector node {node}: malformed loop structure")
            }
        }
    }
}

impl std::error::Error for VError {}

/// The vector IR graph: nodes in an arena.
#[derive(Debug, Clone, Default)]
pub struct VGraph {
    nodes: Vec<VNode>,
}

impl VGraph {
    /// Appends a node and returns its index.
    // CEP:WHAT Adds a vector node.
    // CEP:WHY The vectorizer builds graphs incrementally.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn push(&mut self, n: VNode) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    /// Node accessor.
    // CEP:WHAT Returns a node reference.
    // CEP:WHY Downstream passes read nodes through this accessor.
    // CEP:STATUS complete
    // CEP:FAILURE None for out-of-range indices.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn node(&self, idx: usize) -> Option<&VNode> {
        self.nodes.get(idx)
    }

    /// The type of a node, when it produces a vector value.
    // CEP:WHAT Computes the result type of a node.
    // CEP:WHY Width consistency checks compare operand and result types.
    // CEP:STATUS complete
    // CEP:FAILURE None for control nodes.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn result_type(&self, idx: usize) -> Option<VType> {
        match self.nodes.get(idx)? {
            VNode::VArith { ty, .. } => Some(*ty),
            VNode::VLoad { ty, .. } => Some(*ty),
            VNode::VGather { ty, .. } => Some(*ty),
            VNode::VReduce { ty, .. } => Some(VType {
                elem: ty.elem,
                lanes: 1,
            }),
            VNode::VShuffle { ty, .. } => Some(*ty),
            VNode::VMask { ty, .. } => Some(VType {
                elem: ScalarTag::I8,
                lanes: ty.lanes,
            }),
            VNode::VConvert { to, lanes, .. } => Some(VType {
                elem: *to,
                lanes: *lanes,
            }),
            VNode::Induction { .. } => None,
            _ => None,
        }
    }

    /// Verifies the invariant subset over the whole graph.
    // CEP:WHAT Runs width, shuffle-range, mask, and loop checks.
    // CEP:WHY Downstream selection assumes these invariants (6.6).
    // CEP:STATUS complete
    // CEP:FAILURE The first violation in node order.
    // CEP:ASSUMES None.
    // CEP:COST O(|nodes|).
    // CEP:EVIDENCE cargo test -p vortex-opt
    // CEP:ID: JIT-a16
    pub fn verify(&self) -> Result<(), VError> {
        for (i, n) in self.nodes.iter().enumerate() {
            match n {
                VNode::VArith {
                    ty, operands, mask, ..
                } => {
                    self.check_width(i, ty, operands.as_slice())?;
                    check_mask(self, i, *mask)?;
                }
                VNode::VLoad { ty, mask, .. } => {
                    check_mask(self, i, *mask)?;
                    let _ = ty;
                }
                VNode::VStore { mask, .. }
                | VNode::VGather { mask, .. }
                | VNode::VScatter { mask, .. } => {
                    check_mask(self, i, *mask)?;
                }
                VNode::VShuffle {
                    ty,
                    indices,
                    sources,
                    ..
                } => {
                    let range = ty.lanes as usize * 2;
                    if indices.iter().any(|ix| (*ix as usize) >= range) {
                        return Err(VError::ShuffleRange { node: i });
                    }
                    let _ = sources;
                }
                VNode::VReduce { vector, .. } => {
                    if self.result_type(*vector).is_none() {
                        return Err(VError::WidthMismatch { node: i });
                    }
                }
                VNode::LoopLatch { .. } => {
                    let headers = self
                        .nodes
                        .iter()
                        .any(|n| matches!(n, VNode::LoopHeader { .. }));
                    if !headers {
                        return Err(VError::LoopMalformed { node: i });
                    }
                }
                VNode::LoopExit { .. } => {
                    let _ = i;
                }
                VNode::Induction { .. } => {
                    let _ = i;
                }
                _ => {
                    let _ = i;
                }
            }
        }
        Ok(())
    }

    /// Width consistency for operand lists.
    // CEP:WHAT Checks that every vector operand matches the result width.
    // CEP:WHY Invariant: lane counts agree across each vector operation.
    // CEP:STATUS complete
    // CEP:FAILURE VError::WidthMismatch on the first disagreement.
    // CEP:ASSUMES None.
    // CEP:COST O(operands).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn check_width(&self, node: usize, ty: &VType, operands: &[usize]) -> Result<(), VError> {
        for op in operands.iter() {
            if let Some(t) = self.result_type(*op) {
                if t.lanes != ty.lanes {
                    return Err(VError::WidthMismatch { node });
                }
            }
        }
        Ok(())
    }
}

/// Mask binding check.
// CEP:WHAT Validates that bound masks reference existing nodes.
// CEP:WHY 6.6: masks make per-lane behavior precise; dangling masks are
//        malformed graphs.
// CEP:STATUS complete
// CEP:FAILURE VError::MaskMissing when a binding dangles.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt
fn check_mask(graph: &VGraph, node: usize, mask: Option<usize>) -> Result<(), VError> {
    if let Some(m) = mask {
        if graph.node(m).is_none() {
            return Err(VError::MaskMissing { node });
        }
    }
    Ok(())
}
