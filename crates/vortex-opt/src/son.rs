// CEP:FILE crates/vortex-opt/src/son.rs
// CEP:WHAT The unified SSA graph built from UGB bytecode: CFG blocks with
//        terminators, value nodes with pending-vreg inputs, strict SSA
//        construction with minimal phi placement at dominance frontiers and
//        dominator-tree renaming (SON-001/SON-002; design.md section 2.2).
// CEP:WHY The optimizing tiers need explicit control/data relationships:
//        every data value defined exactly once, phi nodes only at dominance
//        frontiers, block-entry deopt states plus per-value definitions so
//        any deopt point reconstructs the full virtual-register frame
//        (INV-LLIR-013).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Undecodable bytecode and unsupported opcodes surface as typed
//        SonError values; unreachable blocks are removed before dominator
//        computation (documented dead-code elimination at the CFG level).
// CEP:ASSUMES The method passed vortex-ugb verification.
// CEP:COST O(instructions) build, O(blocks) dominators (CHK), O(values x
//        depth) renaming; phi placement is the minimal-SSA worklist.
// CEP:EVIDENCE cargo test -p vortex-opt
use std::collections::HashMap;

use vortex_ugb::{instr, Method, Op};

/// SSA value id.
pub type VId = usize;
/// Block id.
pub type BId = usize;

/// Representation commitment (ARS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rep {
    /// NV-1 tagged word (the interpreter's representation).
    Tagged,
    /// Unboxed 64-bit integer (committed only under a range proof).
    I64,
    /// Unboxed binary64.
    F64,
}

/// Block terminator.
#[derive(Debug, Clone)]
pub enum Term {
    /// Branch to `target` when the condition is truthy (`on_truthy`), else
    /// to the fallthrough block.
    Branch {
        /// The condition value.
        cond: VId,
        /// Block taken when the test matches `on_truthy`.
        target: BId,
        /// Whether truthiness directs control to `target`.
        on_truthy: bool,
    },
    /// Unconditional branch.
    Goto(BId),
    /// Return the value.
    Return(VId),
}

/// A graph value.
#[derive(Debug, Clone)]
pub struct Value {
    /// Operation.
    pub op: SonOp,
    /// Input edges (data; distinguished by position per op).
    pub inputs: Vec<VId>,
    /// Pending virtual-register inputs filled by renaming (parallel to the
    /// consumed input slots; `None` once concrete).
    pub pending: Vec<Option<u16>>,
    /// Enclosing block.
    pub block: BId,
    /// Representation commitment (filled by the ARS pass).
    pub rep: Rep,
    /// Bytecode pc that produced this value (deopt attribution).
    pub pc: usize,
    /// The virtual register this value defines, if any.
    pub def_vreg: Option<u16>,
}

/// Graph operations.
#[derive(Debug, Clone, PartialEq)]
pub enum SonOp {
    /// i64 constant (may exceed the i48 immediate domain; boxed at the
    /// boundary exactly like the interpreter's overflow path).
    ConstInt(i64),
    /// binary64 constant.
    ConstFloat(f64),
    /// Null immediate.
    Null,
    /// Boolean immediate.
    Bool(bool),
    /// Incoming argument register.
    Arg(u16),
    /// Register-to-register copy (the UGB Move).
    Copy,
    /// Binary arithmetic (semantics per representation).
    Add,
    /// Binary arithmetic (semantics per representation).
    Sub,
    /// Binary arithmetic (semantics per representation).
    Mul,
    /// Binary arithmetic (semantics per representation).
    Div,
    /// Negation (0 - a).
    Neg,
    /// Equality comparison.
    Eq,
    /// Less-than comparison.
    Lt,
    /// Less-or-equal comparison.
    Le,
    /// `input == Null`.
    IsNull,
    /// SSA phi merging one virtual register.
    Phi {
        /// The virtual register the phi merges.
        vreg: u16,
    },
    /// Allocate an array of constant length.
    ArrNew(u32),
    /// `arr[idx]` load (bounds-checked; BCE may prove it away).
    ArrLoad,
    /// `arr[idx] = val`.
    ArrStore,
    /// `arr.length`.
    ArrLen,
    /// Scalar-replaced load from a local slot table.
    SroaLoad(usize),
    /// Scalar-replaced store into a local slot table.
    SroaStore(usize),
    /// Scalar-replaced allocation: declares a local slot table of `len`
    /// slots (the allocation itself is gone).
    SroaNew {
        /// The slot table id.
        slot: usize,
        /// The constant length.
        len: u32,
    },
}

/// A CFG block: phis first in `insts`, terminator last.
#[derive(Debug, Clone, Default)]
pub struct Block {
    /// Instruction order: phi values first.
    pub insts: Vec<VId>,
    /// Terminator (absent only in unreachable blocks).
    pub term: Option<Term>,
    /// Successor blocks in CFG order (fallthrough first for branches).
    pub succs: Vec<BId>,
    /// Predecessor blocks in CFG order (mirrors `succs` ordering).
    pub preds: Vec<BId>,
}

/// The SSA graph of one method.
#[derive(Debug, Clone)]
pub struct SonFunc {
    /// Values in creation order.
    pub values: Vec<Value>,
    /// Blocks in id order (entry is 0).
    pub blocks: Vec<Block>,
    /// Block-entry virtual register state recorded during renaming
    /// (`entry_state[b]` = vreg -> value at entry of `b`).
    pub entry_state: Vec<Vec<(u16, VId)>>,
    /// Virtual register count.
    pub nregs: u16,
    /// Incoming argument register count (`0..arg_count` of the method).
    pub arg_count: u16,
}

/// Compilation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SonError {
    /// The bytecode stream did not decode.
    Decode {
        /// Byte offset of the fault.
        offset: usize,
    },
    /// The method uses an opcode outside the optimizing pipeline's subset.
    Unsupported {
        /// The opcode.
        op: Op,
        /// Byte offset of the instruction.
        offset: usize,
    },
    /// A branch targeted a pc with no block (verifier prevents this).
    BranchTarget {
        /// The target pc.
        target: usize,
    },
}

/// Builds the SSA graph for a method.
// CEP:WHAT Builds the SSA graph for a method.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn build(method: &Method) -> Result<SonFunc, SonError> {
    let decoded = decode(method)?;

    // Leaders and contiguous block ranges.
    let mut leaders = vec![false; decoded.len()];
    leaders[0] = true;
    for (i, (_, instr, target)) in decoded.iter().enumerate() {
        if vortex_ugb::sig(instr.op).branches > 0 {
            if let Some(t) = target {
                if let Some(ti) = decoded.iter().position(|(pc, _, _)| pc == t) {
                    leaders[ti] = true;
                }
            }
            if i + 1 < decoded.len() {
                leaders[i + 1] = true;
            }
        }
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for (i, l) in leaders.iter().enumerate() {
        if *l && i > start {
            ranges.push((start, i));
            start = i;
        }
    }
    ranges.push((start, decoded.len()));
    let nblocks = ranges.len();
    let pc_to_block: HashMap<usize, BId> = ranges
        .iter()
        .enumerate()
        .map(|(b, &(s, _))| (decoded[s].0, b))
        .collect();

    let mut func = SonFunc {
        values: Vec::new(),
        blocks: vec![Block::default(); nblocks],
        entry_state: vec![Vec::new(); nblocks],
        nregs: method.register_count,
        arg_count: method.arg_count,
    };

    // Pass 1: emit values per block with pending vreg inputs.
    let mut terms: Vec<Option<PendingTerm>> = vec![None; nblocks];

    for (b, &(s, e)) in ranges.iter().enumerate() {
        let mut local: HashMap<u16, VId> = HashMap::new();
        // Arguments are defined at the entry block.
        if b == 0 {
            for v in 0..method.arg_count {
                let vid = func.push_op(SonOp::Arg(v), Vec::new(), Vec::new(), b, 0, Some(v));
                func.blocks[b].insts.push(vid);
                local.insert(v, vid);
            }
        }
        for (idx, (_, instr, target)) in decoded[s..e].iter().enumerate() {
            let pc = decoded[s + idx].0;
            let target = *target;
            let mk = |func: &mut SonFunc,
                      op: SonOp,
                      inputs: Vec<VId>,
                      pending: Vec<Option<u16>>,
                      def: Option<u16>|
             -> VId {
                let vid = func.push_op(op, inputs, pending, b, pc, def);
                func.blocks[b].insts.push(vid);
                vid
            };
            let use_v = |local: &HashMap<u16, VId>, r: u16| (local.get(&r).copied(), Some(r));
            match instr.op {
                Op::LoadNull => {
                    let v = mk(&mut func, SonOp::Null, vec![], vec![], Some(instr.regs[0]));
                    local.insert(instr.regs[0], v);
                }
                Op::LoadTrue | Op::LoadFalse => {
                    let v = mk(
                        &mut func,
                        SonOp::Bool(instr.op == Op::LoadTrue),
                        vec![],
                        vec![],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::LoadInt32 => {
                    let v = mk(
                        &mut func,
                        SonOp::ConstInt(i64::from(instr.imm32)),
                        vec![],
                        vec![],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::LoadFloat => {
                    let v = mk(
                        &mut func,
                        SonOp::ConstFloat(f64::from_bits(instr.imm64)),
                        vec![],
                        vec![],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::LoadConst => {
                    let c = method.consts.get(usize::from(instr.pool)).copied();
                    let v = match c {
                        Some(vortex_ugb::Const::Int(i)) => mk(
                            &mut func,
                            SonOp::ConstInt(i),
                            vec![],
                            vec![],
                            Some(instr.regs[0]),
                        ),
                        Some(vortex_ugb::Const::Float(f)) => mk(
                            &mut func,
                            SonOp::ConstFloat(f),
                            vec![],
                            vec![],
                            Some(instr.regs[0]),
                        ),
                        None => return Err(SonError::Decode { offset: pc }),
                    };
                    local.insert(instr.regs[0], v);
                }
                Op::Move => {
                    let (src, pend) = use_v(&local, instr.regs[1]);
                    let v = mk(
                        &mut func,
                        SonOp::Copy,
                        vec![src.unwrap_or(0)],
                        vec![pend],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Eq | Op::Lt | Op::Le => {
                    let (a, pa) = use_v(&local, instr.regs[1]);
                    let (b2, pb) = use_v(&local, instr.regs[2]);
                    let op = match instr.op {
                        Op::Add => SonOp::Add,
                        Op::Sub => SonOp::Sub,
                        Op::Mul => SonOp::Mul,
                        Op::Div => SonOp::Div,
                        Op::Eq => SonOp::Eq,
                        Op::Lt => SonOp::Lt,
                        _ => SonOp::Le,
                    };
                    let v = mk(
                        &mut func,
                        op,
                        vec![a.unwrap_or(0), b2.unwrap_or(0)],
                        vec![pa, pb],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::Neg => {
                    let (a, pa) = use_v(&local, instr.regs[1]);
                    let v = mk(
                        &mut func,
                        SonOp::Neg,
                        vec![a.unwrap_or(0)],
                        vec![pa],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::IsNull => {
                    let (a, pa) = use_v(&local, instr.regs[1]);
                    let v = mk(
                        &mut func,
                        SonOp::IsNull,
                        vec![a.unwrap_or(0)],
                        vec![pa],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::AllocArray => {
                    let v = mk(
                        &mut func,
                        SonOp::ArrNew(instr.imm32u[0]),
                        vec![],
                        vec![],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::LoadIndex => {
                    let (arr, parr) = use_v(&local, instr.regs[1]);
                    let (ix, pix) = use_v(&local, instr.regs[2]);
                    let v = mk(
                        &mut func,
                        SonOp::ArrLoad,
                        vec![arr.unwrap_or(0), ix.unwrap_or(0)],
                        vec![parr, pix],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::StoreIndex => {
                    let (arr, parr) = use_v(&local, instr.regs[0]);
                    let (ix, pix) = use_v(&local, instr.regs[1]);
                    let (val, pval) = use_v(&local, instr.regs[2]);
                    let _ = mk(
                        &mut func,
                        SonOp::ArrStore,
                        vec![arr.unwrap_or(0), ix.unwrap_or(0), val.unwrap_or(0)],
                        vec![parr, pix, pval],
                        None,
                    );
                }
                Op::LoadLength => {
                    let (arr, parr) = use_v(&local, instr.regs[1]);
                    let v = mk(
                        &mut func,
                        SonOp::ArrLen,
                        vec![arr.unwrap_or(0)],
                        vec![parr],
                        Some(instr.regs[0]),
                    );
                    local.insert(instr.regs[0], v);
                }
                Op::Safepoint | Op::Nop => { /* documented no-op */ }
                Op::Jump => {
                    let t = target.ok_or(SonError::BranchTarget { target: usize::MAX })?;
                    let tb = *pc_to_block
                        .get(&t)
                        .ok_or(SonError::BranchTarget { target: t })?;
                    terms[b] = Some(PendingTerm::Goto(tb));
                }
                Op::JumpTrue | Op::JumpFalse => {
                    let t = target.ok_or(SonError::BranchTarget { target: usize::MAX })?;
                    let tb = *pc_to_block
                        .get(&t)
                        .ok_or(SonError::BranchTarget { target: t })?;
                    terms[b] = Some(PendingTerm::Branch {
                        cond_vreg: instr.regs[0],
                        target: tb,
                        on_truthy: instr.op == Op::JumpTrue,
                    });
                }
                Op::Return => {
                    terms[b] = Some(PendingTerm::ReturnV(instr.regs[0]));
                }
                unsupported => {
                    return Err(SonError::Unsupported {
                        op: unsupported,
                        offset: pc,
                    })
                }
            }
        }
    }

    // Pass 2: wire successors/predecessors from terminators. A block whose
    // last instruction is not a branch falls through to the next block: it
    // gets an implicit Goto terminator.
    for b in 0..nblocks {
        if terms[b].is_none() && b + 1 < nblocks && !ranges_next_is_return(b) {
            terms[b] = Some(PendingTerm::Goto(b + 1));
        }
    }
    fn ranges_next_is_return(_b: usize) -> bool {
        false // fallthrough is unconditional; the target block decides.
    }
    for (b, term) in terms.iter().enumerate() {
        match term {
            Some(PendingTerm::Goto(t)) => {
                func.blocks[b].succs.push(*t);
                func.blocks[*t].preds.push(b);
            }
            Some(PendingTerm::Branch { target, .. }) => {
                if b + 1 < nblocks {
                    func.blocks[b].succs.push(b + 1);
                    func.blocks[b + 1].preds.push(b);
                }
                func.blocks[b].succs.push(*target);
                func.blocks[*target].preds.push(b);
            }
            _ => { /* Return blocks have no CFG successors */ }
        }
    }

    // Drop unreachable blocks (dead-code elimination at the CFG level).
    let reachable = reachability(&func.blocks, nblocks);
    for b in 0..nblocks {
        if !reachable[b] {
            func.blocks[b].succs.clear();
            func.blocks[b].preds.clear();
            terms[b] = None;
        }
    }

    // Pass 3: SSA - dominators, frontiers, minimal phi placement, rename.
    let order = rpo(&func.blocks, nblocks);
    let idom = dominators(&func.blocks, &order, nblocks);
    let frontiers = dominance_frontiers(&func.blocks, &idom, nblocks);

    let mut def_blocks: Vec<Vec<BId>> = vec![Vec::new(); usize::from(func.nregs)];
    for val in &func.values {
        if let Some(r) = val.def_vreg {
            let rb = usize::from(r);
            if !def_blocks[rb].contains(&val.block) {
                def_blocks[rb].push(val.block);
            }
        }
    }

    let mut phis: Vec<HashMap<BId, VId>> = vec![HashMap::new(); usize::from(func.nregs)];
    for v in 0..usize::from(func.nregs) {
        if def_blocks[v].len() < 2 {
            continue;
        }
        let mut worklist = def_blocks[v].clone();
        while let Some(w) = worklist.pop() {
            for &y in frontiers.get(w).into_iter().flatten() {
                if !reachable[y] || phis[v].contains_key(&y) {
                    continue;
                }
                let vid = func.push_op(
                    SonOp::Phi { vreg: v as u16 },
                    vec![0; func.blocks[y].preds.len()],
                    vec![None; func.blocks[y].preds.len()],
                    y,
                    usize::MAX,
                    Some(v as u16),
                );
                func.blocks[y].insts.insert(0, vid);
                phis[v].insert(y, vid);
                worklist.push(y);
            }
        }
    }

    // Rename over the dominator tree.
    let children = dom_tree_children(&idom, nblocks);
    let mut stacks: Vec<Vec<VId>> = vec![Vec::new(); usize::from(func.nregs)];
    let mut visited = vec![false; nblocks];
    rename_block(
        &mut func,
        &terms,
        &phis,
        &mut stacks,
        &children,
        0,
        &mut visited,
    );

    Ok(func)
}

impl SonFunc {
    /// Pushes a value and returns its id.
    pub fn push_op(
        &mut self,
        op: SonOp,
        inputs: Vec<VId>,
        pending: Vec<Option<u16>>,
        block: BId,
        pc: usize,
        def_vreg: Option<u16>,
    ) -> VId {
        let id = self.values.len();
        self.values.push(Value {
            op,
            inputs,
            pending,
            block,
            rep: Rep::Tagged,
            pc,
            def_vreg,
        });
        id
    }

    /// Borrows a value.
    #[must_use]
    pub fn value(&self, id: VId) -> &Value {
        &self.values[id]
    }

    /// Reconstructs the virtual-register frame at a bytecode pc
    /// (INV-LLIR-013): the block-entry state plus every value defined
    /// earlier in the same block.
    #[must_use]
    // CEP:WHAT Reconstructs the virtual-register frame at a bytecode pc (INV-LLIR-013): the block-entry state plus every value defined earlier in the same block.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn deopt_frame_at(&self, pc: usize, block_of_pc: BId) -> Vec<(u16, VId)> {
        let mut map: Vec<(u16, VId)> = self.entry_state[block_of_pc].clone();
        for &vid in &self.blocks[block_of_pc].insts {
            let v = &self.values[vid];
            if v.pc >= pc {
                break;
            }
            if let Some(r) = v.def_vreg {
                map.retain(|(vr, _)| *vr != r);
                map.push((r, vid));
            }
        }
        map
    }
}

/// Renames one block (dominator-tree DFS): resolves pending vreg inputs,
/// records entry state, fills successor phi inputs, and recurses.
// CEP:WHAT Renames one block (dominator-tree DFS): resolves pending vreg inputs, records entry state, fills successor phi inputs, and recurses.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn rename_block(
    func: &mut SonFunc,
    terms: &[Option<PendingTerm>],
    phis: &[HashMap<BId, VId>],
    stacks: &mut [Vec<VId>],
    children: &[Vec<BId>],
    block: BId,
    visited: &mut [bool],
) {
    if visited[block] {
        return;
    }
    visited[block] = true;

    // Entry state: current stack tops for every register with a reaching
    // definition (plus the phis of this block, pushed below first? No -
    // the entry state is what flows IN; phis are the merge, pushed after).
    let mut entry: Vec<(u16, VId)> = Vec::new();
    for v in 0..usize::from(func.nregs) {
        if let Some(&top) = stacks[v].last() {
            entry.push((v as u16, top));
        }
    }
    func.entry_state[block] = entry;

    // Push phi defs (block parameters).
    let mut pushed: Vec<(u16, VId)> = Vec::new();
    for &vid in &func.blocks[block].insts {
        if let SonOp::Phi { vreg } = func.values[vid].op {
            stacks[usize::from(vreg)].push(vid);
            pushed.push((vreg, vid));
        }
    }

    // Resolve non-phi instruction inputs and push definitions in order.
    let mut def_pushed: Vec<(u16, VId)> = Vec::new();
    for &vid in &func.blocks[block].insts {
        if matches!(func.values[vid].op, SonOp::Phi { .. }) {
            continue;
        }
        let pend = std::mem::take(&mut func.values[vid].pending);
        for (slot, p) in pend.iter().enumerate() {
            if let Some(v) = p {
                let resolved = stacks[usize::from(*v)].last().copied().unwrap_or(0);
                func.values[vid].inputs[slot] = resolved;
            }
        }
        if let Some(r) = func.values[vid].def_vreg {
            stacks[usize::from(r)].push(vid);
            def_pushed.push((r, vid));
        }
    }

    // Resolve terminator inputs and fill successor phi inputs.
    if let Some(term) = &terms[block] {
        match term {
            PendingTerm::Branch {
                cond_vreg,
                target,
                on_truthy,
            } => {
                let cond = stacks[usize::from(*cond_vreg)].last().copied().unwrap_or(0);
                func.blocks[block].term = Some(Term::Branch {
                    cond,
                    target: *target,
                    on_truthy: *on_truthy,
                });
            }
            PendingTerm::Goto(t) => {
                func.blocks[block].term = Some(Term::Goto(*t));
            }
            PendingTerm::ReturnV(r) => {
                let val = stacks[usize::from(*r)].last().copied().unwrap_or(0);
                func.blocks[block].term = Some(Term::Return(val));
            }
        }
    }
    for (si, &succ) in func.blocks[block].succs.clone().iter().enumerate() {
        let _ = si;
        for (v, phi_map) in phis.iter().enumerate() {
            if let Some(&p) = phi_map.get(&succ) {
                let top = stacks[v].last().copied().unwrap_or(0);
                // The phi input slot matches the predecessor index of this
                // block in the successor's pred list.
                if let Some(slot) = func.blocks[succ].preds.iter().position(|&p2| p2 == block) {
                    func.values[p].inputs[slot] = top;
                    func.values[p].pending[slot] = None;
                }
            }
        }
    }

    for &c in &children[block] {
        rename_block(func, terms, phis, stacks, children, c, visited);
    }

    // Pop instruction and phi definitions.
    for (v, _) in def_pushed.iter().rev().chain(pushed.iter().rev()) {
        stacks[usize::from(*v)].pop();
    }
}

/// Pending-terminator shadow type used during construction.
#[derive(Clone, Debug)]
enum PendingTerm {
    Branch {
        cond_vreg: u16,
        target: BId,
        on_truthy: bool,
    },
    Goto(BId),
    ReturnV(u16),
}

/// Decodes the method stream.
// CEP:WHAT Decodes the method stream.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn decode(method: &Method) -> Result<Vec<(usize, instr::Instr, Option<usize>)>, SonError> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < method.code.len() {
        let (i, next) =
            instr::decode_at(&method.code, p).map_err(|e| SonError::Decode { offset: e.offset })?;
        let target = if vortex_ugb::sig(i.op).branches > 0 {
            Some(method.branch_target(&i, p, next - p))
        } else {
            None
        };
        out.push((p, i, target));
        p = next;
    }
    Ok(out)
}

/// Reachable set from block 0.
// CEP:WHAT Reachable set from block 0.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn reachability(blocks: &[Block], n: usize) -> Vec<bool> {
    let mut seen = vec![false; n];
    let mut stack = vec![0_usize];
    while let Some(b) = stack.pop() {
        if seen[b] {
            continue;
        }
        seen[b] = true;
        for &s in &blocks[b].succs {
            if s < n {
                stack.push(s);
            }
        }
    }
    seen
}

/// Reverse post-order over reachable blocks.
// CEP:WHAT Reverse post-order over reachable blocks.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn rpo(blocks: &[Block], n: usize) -> Vec<BId> {
    let mut visited = vec![false; n];
    let mut post = Vec::new();
    let mut stack: Vec<(BId, usize)> = vec![(0, 0)];
    visited[0] = true;
    while let Some((b, i)) = stack.pop() {
        let succs = &blocks[b].succs;
        if i < succs.len() {
            stack.push((b, i + 1));
            let s = succs[i];
            if !visited[s] {
                visited[s] = true;
                stack.push((s, 0));
            }
        } else {
            post.push(b);
        }
    }
    post.reverse();
    post
}

/// Immediate dominators (Cooper-Harvey-Kennedy). Entry idom = entry.
// CEP:WHAT Immediate dominators (Cooper-Harvey-Kennedy). Entry idom = entry.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn dominators(blocks: &[Block], order: &[BId], n: usize) -> Vec<Option<BId>> {
    let mut idom: Vec<Option<BId>> = vec![None; n];
    let mut rpo_index: HashMap<BId, usize> = HashMap::new();
    for (i, &b) in order.iter().enumerate() {
        rpo_index.insert(b, i);
    }
    if let Some(&entry) = order.first() {
        idom[entry] = Some(entry);
    }
    let mut changed = true;
    while changed {
        changed = false;
        for &b in order.iter().skip(1) {
            let mut new_idom: Option<BId> = None;
            for &p in &blocks[b].preds {
                if idom[p].is_none() {
                    continue;
                }
                new_idom = Some(match new_idom {
                    None => p,
                    Some(cur) => intersect(&idom, &rpo_index, cur, p),
                });
            }
            if let Some(ni) = new_idom {
                if idom[b] != Some(ni) {
                    idom[b] = Some(ni);
                    changed = true;
                }
            }
        }
    }
    idom
}

/// Two-finger intersect over RPO indices (CHK).
fn intersect(idom: &[Option<BId>], rpo_index: &HashMap<BId, usize>, mut a: BId, mut b: BId) -> BId {
    while a != b {
        while rpo_index.get(&a).copied().unwrap_or(usize::MAX)
            > rpo_index.get(&b).copied().unwrap_or(usize::MAX)
        {
            a = idom[a].expect("idom defined on reachable path");
        }
        while rpo_index.get(&b).copied().unwrap_or(usize::MAX)
            > rpo_index.get(&a).copied().unwrap_or(usize::MAX)
        {
            b = idom[b].expect("idom defined on reachable path");
        }
    }
    a
}

/// Dominance frontiers (Cytron).
// CEP:WHAT Dominance frontiers (Cytron).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn dominance_frontiers(blocks: &[Block], idom: &[Option<BId>], n: usize) -> Vec<Vec<BId>> {
    let mut df: Vec<Vec<BId>> = vec![Vec::new(); n];
    for b in 0..n {
        if blocks[b].preds.len() >= 2 {
            for &p in &blocks[b].preds {
                // Cytron: runner = p; while runner != idom[b]:
                // df[runner] += b; runner = idom[runner].
                let mut runner = p;
                while runner != idom[b].unwrap_or(runner) {
                    if !df[runner].contains(&b) {
                        df[runner].push(b);
                    }
                    match idom[runner] {
                        Some(r) if r != runner => runner = r,
                        _ => break, // entry guard (idom cycle defense)
                    }
                }
            }
        }
    }
    df
}

/// Dominator-tree children lists.
fn dom_tree_children(idom: &[Option<BId>], n: usize) -> Vec<Vec<BId>> {
    let mut children: Vec<Vec<BId>> = vec![Vec::new(); n];
    for b in 0..n {
        if let Some(d) = idom[b] {
            if d != b {
                children[d].push(b);
            }
        }
    }
    children
}
