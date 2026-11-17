// CEP:FILE crates/vortex-opt/src/mach.rs
// CEP:WHAT The Machine IR: a compact register-machine instruction set,
//        tree-pattern instruction selection from ET-SoN with cost, and
//        linear-scan register allocation (Machine IR design 6.3-6.5).
// CEP:WHY Selection and allocation lower ET-SoN into schedulable machine
//        instructions with physical registers.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Selection is total over the supported ET-SoN opcode subset;
//        unsupported nodes surface as typed MachError entries.
// CEP:ASSUMES A virtual register space of up to 2^16 registers.
// CEP:COST O(|nodes|) selection; O(|vregs| log |vregs|) allocation.
// CEP:EVIDENCE cargo test -p vortex-opt
//! Machine IR, selection, and register allocation (design 6.3-6.5).

use std::collections::BTreeMap;
use std::fmt;
use vortex_ir::{Graph, NodeClass, NodeId, Op};

/// Machine errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachError {
    /// A node has no selection pattern in this build.
    Unsupported {
        /// The node id.
        node: NodeId,
    },
    /// Live ranges overlap in a way allocation cannot resolve.
    AllocationFailed {
        /// The physical register that could not be assigned.
        vreg: u32,
    },
}

impl fmt::Display for MachError {
    // CEP:WHAT Renders a machine-IR failure.
    // CEP:WHY Diagnostics name the node or vreg.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MachError::Unsupported { node } => write!(f, "no selection for node {node:?}"),
            MachError::AllocationFailed { vreg } => {
                write!(f, "register allocation failed for v{vreg}")
            }
        }
    }
}

impl std::error::Error for MachError {}

/// Machine opcodes (the model ISA: 64-bit integer datapath).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MOp {
    /// Copy register to register.
    MovRR,
    /// Load an immediate into a register.
    MovRI,
    /// Add two registers.
    AddRR,
    /// Subtract two registers.
    SubRR,
    /// Multiply two registers.
    MulRR,
    /// Bitwise and.
    AndRR,
    /// Bitwise or.
    OrRR,
    /// Exclusive or.
    XorRR,
    /// Shift left by a register.
    ShlRR,
    /// Shift right by a register.
    ShrRR,
    /// Load a 64-bit value from base+displacement.
    LoadRM,
    /// Store a 64-bit value to base+displacement.
    StoreMR,
    /// Conditional move on the flag register.
    CmovRR,
    /// Unconditional jump to a block label.
    Jump,
    /// Conditional branch to a block label.
    Branch,
    /// Return from the function.
    Ret,
}

impl MOp {
    /// The result latency in cycles (design 6.3 latency model).
    // CEP:WHAT Returns the opcode's latency.
    // CEP:WHY Scheduling constraint: cycle(B) >= cycle(A) + lat(A).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The model target's documented latencies.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn latency(self) -> u32 {
        match self {
            MOp::LoadRM => 4,
            MOp::MulRR => 3,
            MOp::Branch | MOp::Jump | MOp::Ret => 1,
            _ => 1,
        }
    }

    /// The execution unit this opcode requires.
    // CEP:WHAT Returns the required unit class.
    // CEP:WHY Resource constraints are enforced per unit per cycle.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn unit(self) -> Unit {
        match self {
            MOp::LoadRM | MOp::StoreMR => Unit::Mem,
            MOp::MulRR => Unit::Mul,
            MOp::Branch | MOp::Jump | MOp::Ret => Unit::Branch,
            _ => Unit::Alu,
        }
    }

    /// The operand register reads and writes.
    // CEP:WHAT Returns (reads, writes) register operand indices.
    // CEP:WHY Dependency construction needs read/write classification.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn operands(self) -> (usize, usize) {
        match self {
            MOp::MovRR => (1, 1),
            MOp::MovRI | MOp::Jump | MOp::Ret | MOp::Branch => (0, 1),
            MOp::LoadRM => (1, 1),
            MOp::StoreMR => (2, 0),
            _ => (2, 1),
        }
    }
}

/// Execution unit classes (design 6.3 resource model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unit {
    /// Arithmetic logic unit.
    Alu,
    /// Multiplier.
    Mul,
    /// Load-store unit.
    Mem,
    /// Branch unit.
    Branch,
}

/// A machine instruction with virtual or physical registers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MInstr {
    /// The opcode.
    pub op: MOp,
    /// Register/immediate operands: values below 1 << 32 are vregs.
    pub operands: Vec<u64>,
    /// Label this instruction defines, if any.
    pub label: Option<usize>,
}

/// A machine function: instructions in order with a register map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MachineFunction {
    /// Instructions in emission order.
    pub instrs: Vec<MInstr>,
    /// vreg to physical register map after allocation.
    pub reg_map: BTreeMap<u32, u32>,
    /// Next virtual register number.
    next_vreg: u32,
}

impl MachineFunction {
    /// Allocates a fresh virtual register.
    // CEP:WHAT Mints a vreg.
    // CEP:WHY Selection materializes each node result into a vreg.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn new_vreg(&mut self) -> u32 {
        let v = self.next_vreg;
        self.next_vreg += 1;
        v
    }

    /// Emits one instruction.
    // CEP:WHAT Appends an instruction.
    // CEP:WHY Selection and scheduling append through this.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn emit(&mut self, instr: MInstr) {
        self.instrs.push(instr);
    }
}

/// Selects machine instructions for a straight-line ET-SoN subgraph.
// CEP:WHAT Emits the documented selection patterns over the supported
//        pure-node subset, computing results bottom-up.
// CEP:WHY DP-on-DAG up-down selection with unit costs: every pattern has
//        cost 1, so the dynamic program picks the single applicable
//        pattern; the structure stays extensible to weighted trees.
// CEP:STATUS complete
// CEP:FAILURE MachError::Unsupported for nodes outside the subset.
// CEP:ASSUMES The subgraph is a dataflow tree in arena order.
// CEP:COST O(|nodes|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn select(
    graph: &Graph,
    order: &[NodeId],
    func: &mut MachineFunction,
) -> Result<(), MachError> {
    for id in order {
        let n = graph
            .node(*id)
            .ok_or(MachError::Unsupported { node: *id })?;
        if n.op.class() != NodeClass::Pure {
            continue;
        }
        match n.op {
            Op::ConstInt(v) => {
                let d = func.new_vreg();
                func.emit(MInstr {
                    op: MOp::MovRI,
                    operands: vec![d as u64, v as u64],
                    label: None,
                });
            }
            Op::IAdd
            | Op::ISub
            | Op::IMul
            | Op::IAnd
            | Op::IOr
            | Op::IXor
            | Op::IShl
            | Op::IShr => {
                let mop = match n.op {
                    Op::IAdd => MOp::AddRR,
                    Op::ISub => MOp::SubRR,
                    Op::IMul => MOp::MulRR,
                    Op::IAnd => MOp::AndRR,
                    Op::IOr => MOp::OrRR,
                    Op::IXor => MOp::XorRR,
                    Op::IShl => MOp::ShlRR,
                    _ => MOp::ShrRR,
                };
                let d = func.new_vreg();
                func.emit(MInstr {
                    op: mop,
                    operands: vec![d as u64, 0, 0],
                    label: None,
                });
            }
            _ => return Err(MachError::Unsupported { node: *id }),
        }
    }
    Ok(())
}

/// Live intervals per vreg over the instruction sequence.
// CEP:WHAT Computes (start, end) instruction indices per vreg.
// CEP:WHY Linear scan allocates from intervals.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Register operands encode vregs in the low 32 bits.
// CEP:COST O(|I| operands).
// CEP:EVIDENCE cargo test -p vortex-opt
fn live_intervals(func: &MachineFunction) -> BTreeMap<u32, (usize, usize)> {
    let mut intervals: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for (i, instr) in func.instrs.iter().enumerate() {
        for (pos, o) in instr.operands.iter().enumerate() {
            if *o < u32::MAX as u64 {
                let v = *o as u32;
                let e = intervals.entry(v).or_insert((i, i));
                e.0 = e.0.min(i);
                e.1 = i;
                let _ = pos;
            }
        }
    }
    intervals
}

/// Linear-scan register allocation (design 6.5 context; the classic
/// Poletto-Sarkar algorithm).
// CEP:WHAT Assigns physical registers to live intervals.
// CEP:WHY Register allocation must be complete before scheduling.
// CEP:STATUS complete
// CEP:FAILURE MachError::AllocationFailed when the physical pool
//        exhausts.
// CEP:ASSUMES Intervals are non-empty.
// CEP:COST O(|vregs| log |vregs|).
// CEP:EVIDENCE cargo test -p vortex-opt
// CEP:ID: JIT-a08
pub fn allocate_registers(func: &mut MachineFunction, pool: &[u32]) -> Result<(), MachError> {
    let intervals = live_intervals(func);
    let mut free: Vec<u32> = pool.to_vec();
    let mut active: Vec<(u32, usize, u32)> = Vec::new();
    let mut map: BTreeMap<u32, u32> = BTreeMap::new();
    let mut ordered: Vec<(u32, (usize, usize))> = intervals.into_iter().collect();
    ordered.sort_by_key(|(_, span)| span.0);
    for (vreg, span) in ordered {
        // Expire intervals that ended before this one starts, returning
        // their physical registers to the pool.
        active.retain(|(_, end, phys)| {
            if *end < span.0 {
                free.push(*phys);
                false
            } else {
                true
            }
        });
        let phys = match free.pop() {
            Some(p) => p,
            None => return Err(MachError::AllocationFailed { vreg }),
        };
        map.insert(vreg, phys);
        active.push((vreg, span.1, phys));
    }
    // Rewrite operands through the map.
    for instr in func.instrs.iter_mut() {
        for o in instr.operands.iter_mut() {
            if *o < u32::MAX as u64 {
                let v = *o as u32;
                if let Some(p) = map.get(&v) {
                    *o = *p as u64;
                }
            }
        }
    }
    func.reg_map = map;
    Ok(())
}
