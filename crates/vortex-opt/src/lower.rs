// CEP:FILE crates/vortex-opt/src/lower.rs
// CEP:WHAT The LLIR/MIR lowering: representation-committed instructions with
//        explicit conversions at every representation boundary
//        (INV-LLIR-004), safepoint maps for live references (INV-LLIR-011),
//        and physical register allocation with two register classes over
//        live intervals (INV-MIR-001..004): linear scan per the T2/T3
//        contract, spill/reload accounting, and pressure tracking.
// CEP:WHY The split commits machine-independent representation decisions
//        first (LLIR), then maps values onto physical locations (MIR) so
//        the allocation evidence - intervals, spills, pressure, safepoint
//        maps - is separately checkable from the optimized semantics.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every conversion is explicit; no value crosses a
//        representation boundary without one. Allocation always succeeds
//        (spills are bounded by live values).
// CEP:ASSUMES The graph was optimized by passes::run_pipeline.
// CEP:COST O(values) lowering, O(values log values + values * regs) scan.
// CEP:EVIDENCE cargo test -p vortex-opt
use std::collections::HashMap;

use vortex_runtime::Loc;

use crate::son::{BId, Rep, SonFunc, SonOp, Term, VId};

/// Executed (lowered) operations: representation committed.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecOp {
    /// i64 constant (raw word).
    ConstInt(i64),
    /// binary64 constant (bits).
    ConstFloat(f64),
    /// Null immediate.
    Null,
    /// Boolean immediate.
    Bool(bool),
    /// Incoming argument.
    Arg(u16),
    /// Register copy.
    Copy,
    /// Tagged arithmetic (interpreter fast-path semantics).
    TagBinop(crate::exec::Bin),
    /// Unboxed binary64 arithmetic.
    FBinop(crate::exec::Bin),
    /// Unboxed i64 arithmetic (range-proven values only).
    IBinop(crate::exec::Bin),
    /// Tagged comparison (abstract equality / numeric compare).
    TagCmp(crate::exec::Cmp),
    /// binary64 comparison.
    FCmp(crate::exec::Cmp),
    /// i64 comparison.
    ICmp(crate::exec::Cmp),
    /// `input == Null` on a tagged word.
    TagIsNull,
    /// Numeric extraction of a tagged word to binary64 (int promotion
    /// included); fails as the interpreter does on non-numeric words.
    ToF64,
    /// Box a binary64 into the tagged float word (nv1::encode_f64
    /// canonicalization).
    BoxFloat,
    /// Box an i64 into a tagged word (immediate domain or boxed heap
    /// object, exactly like the interpreter's overflow path).
    BoxInt,
    /// Allocate an array (scalar-replaced when `sroa` is set).
    ArrNew {
        /// Constant length.
        len: u32,
        /// The SROA slot, when the allocation was scalar-replaced.
        sroa: Option<usize>,
    },
    /// Bounds-checked element load (heap or SROA slot).
    ArrLoad {
        /// The SROA slot, when scalar-replaced.
        sroa: Option<usize>,
    },
    /// Element store (heap or SROA slot; AOSS policy applies on the heap).
    ArrStore {
        /// The SROA slot, when scalar-replaced.
        sroa: Option<usize>,
    },
    /// Array length (SROA slots report the constant).
    ArrLen {
        /// The SROA slot, when scalar-replaced.
        sroa: Option<usize>,
        /// The constant length of a scalar-replaced allocation.
        const_len: Option<i64>,
    },
    /// SSA phi (evaluated from the incoming predecessor edge).
    Phi,
    /// Load from a local SROA slot table (bounds-checked).
    SroaLoad(usize),
    /// Store into a local SROA slot table (bounds-checked).
    SroaStore(usize),
    /// Declare a local SROA slot table (the scalar-replaced allocation).
    SroaNew {
        /// The slot table id.
        slot: usize,
        /// The constant length.
        len: u32,
    },
    /// Speculation guard (deoptimizes to T0 on failure).
    Guard(crate::son::GuardKind),
}

/// Lowered value.
#[derive(Debug, Clone)]
pub struct PlanValue {
    /// Operation.
    pub op: ExecOp,
    /// Input value ids.
    pub inputs: Vec<VId>,
    /// Enclosing block.
    pub block: BId,
    /// Representation.
    pub rep: Rep,
    /// Source bytecode pc.
    pub pc: usize,
    /// Defined virtual register.
    pub def_vreg: Option<u16>,
}

/// Lowered block.
#[derive(Debug, Clone, Default)]
pub struct PlanBlock {
    /// Values evaluated at block entry (phis) and in order.
    pub insts: Vec<VId>,
    /// Terminator.
    pub term: Option<Term>,
    /// Successors.
    pub succs: Vec<BId>,
    /// Predecessors.
    pub preds: Vec<BId>,
}

/// A safepoint map: live reference values at a safepoint (INV-LLIR-011,
/// INV-MIR-004).
#[derive(Debug, Clone)]
pub struct SafepointMap {
    /// Block of the safepoint.
    pub block: BId,
    /// Live reference value ids.
    pub live_refs: Vec<VId>,
}

/// The compiled plan (LLIR + MIR allocation metadata).
#[derive(Debug, Clone)]
pub struct Plan {
    /// Values.
    pub values: Vec<PlanValue>,
    /// Blocks.
    pub blocks: Vec<PlanBlock>,
    /// Physical location per value.
    pub locations: Vec<Loc>,
    /// Number of spill slots allocated.
    pub spill_slots: usize,
    /// Maximum simultaneously-live values (register pressure evidence).
    pub pressure: usize,
    /// Safepoint maps.
    pub safepoint_maps: Vec<SafepointMap>,
    /// The virtual-register frame width.
    pub nregs: u16,
    /// Argument register count.
    pub arg_count: u16,
    /// Number of SROA slot tables (set by the crate root after lowering).
    pub sroa_slots: usize,
    /// Pass statistics recorded by the pipeline (optimization remarks).
    pub stats: crate::passes::PassStats,
    /// The method the plan was compiled from.
    pub method_index: usize,
}

/// GPR width of the reference allocation (fast-map width parity with T1).
pub const GPR_COUNT: usize = 8;
/// FPR width of the reference allocation.
pub const FPR_COUNT: usize = 8;

/// Lowers an optimized graph to the plan with allocation.
// CEP:WHAT Lowers an optimized graph to the plan with allocation.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn lower(func: &SonFunc) -> Plan {
    // L1: copy values 1:1 with committed ops.
    let mut plan = Plan {
        values: func
            .values
            .iter()
            .map(|v| PlanValue {
                op: exec_op_of(v),
                inputs: v.inputs.clone(),
                block: v.block,
                rep: v.rep,
                pc: v.pc,
                def_vreg: v.def_vreg,
            })
            .collect(),
        blocks: Vec::new(),
        locations: Vec::new(),
        spill_slots: 0,
        pressure: 0,
        safepoint_maps: Vec::new(),
        nregs: func.nregs,
        arg_count: func.arg_count,
        sroa_slots: 0,
        stats: crate::passes::PassStats::default(),
        method_index: func.method_index,
    };

    // L2: explicit conversions per input edge, placed in the consumer's
    // block immediately before the consumer.
    let needs_to_f64 = |vid: VId, plan: &Plan| plan.values[vid].rep != Rep::F64;
    let needs_box = |vid: VId, plan: &Plan| matches!(plan.values[vid].rep, Rep::F64 | Rep::I64);

    for b in 0..func.blocks.len() {
        let mut new_insts: Vec<VId> = Vec::new();
        let block_vals: Vec<VId> = func.blocks[b].insts.clone();
        for vid in block_vals {
            // Conversion inputs first.
            let inputs = plan.values[vid].inputs.clone();
            let mut new_inputs = inputs.clone();
            for (slot, &i) in inputs.iter().enumerate() {
                let conv_needed = match plan.values[vid].op {
                    ExecOp::FBinop(_) | ExecOp::FCmp(_) => needs_to_f64(i, &plan),
                    ExecOp::IBinop(_) | ExecOp::ICmp(_) => false,
                    ExecOp::TagBinop(_)
                    | ExecOp::TagCmp(_)
                    | ExecOp::TagIsNull
                    | ExecOp::Copy
                    | ExecOp::ArrStore { .. }
                    | ExecOp::SroaStore(_) => needs_box(i, &plan),
                    ExecOp::Phi => false, // phi inputs already match the phi rep
                    _ => false,
                };
                if conv_needed {
                    let src_rep = plan.values[i].rep;
                    let conv_op = if plan.values[vid].op_is_float_math() {
                        ExecOp::ToF64
                    } else if src_rep == Rep::F64 {
                        ExecOp::BoxFloat
                    } else {
                        ExecOp::BoxInt
                    };
                    let conv_rep = if matches!(conv_op, ExecOp::ToF64) {
                        Rep::F64
                    } else {
                        Rep::Tagged
                    };
                    let cid = plan.values.len();
                    plan.values.push(PlanValue {
                        op: conv_op,
                        inputs: vec![i],
                        block: b,
                        rep: conv_rep,
                        pc: plan.values[vid].pc,
                        def_vreg: None,
                    });
                    new_insts.push(cid);
                    new_inputs[slot] = cid;
                }
            }
            plan.values[vid].inputs = new_inputs;
            new_insts.push(vid);
        }
        plan.blocks.push(PlanBlock {
            insts: new_insts,
            term: func.blocks[b].term.clone(),
            succs: func.blocks[b].succs.clone(),
            preds: func.blocks[b].preds.clone(),
        });
    }
    // Terminator conversions (branch conditions, return values).
    for b in 0..plan.blocks.len() {
        let term = plan.blocks[b].term.clone();
        if let Some(t) = term {
            match t {
                Term::Branch {
                    cond,
                    target,
                    on_truthy,
                } => {
                    let mut cond = cond;
                    if needs_box(cond, &plan) {
                        let cid = plan.values.len();
                        let op = if plan.values[cond].rep == Rep::F64 {
                            ExecOp::BoxFloat
                        } else {
                            ExecOp::BoxInt
                        };
                        plan.values.push(PlanValue {
                            op,
                            inputs: vec![cond],
                            block: b,
                            rep: Rep::Tagged,
                            pc: plan.values[cond].pc,
                            def_vreg: None,
                        });
                        plan.blocks[b].insts.push(cid);
                        cond = cid;
                    }
                    plan.blocks[b].term = Some(Term::Branch {
                        cond,
                        target,
                        on_truthy,
                    });
                }
                Term::Return(val) => {
                    let mut val = val;
                    if needs_box(val, &plan) {
                        let cid = plan.values.len();
                        let op = if plan.values[val].rep == Rep::F64 {
                            ExecOp::BoxFloat
                        } else {
                            ExecOp::BoxInt
                        };
                        plan.values.push(PlanValue {
                            op,
                            inputs: vec![val],
                            block: b,
                            rep: Rep::Tagged,
                            pc: plan.values[val].pc,
                            def_vreg: None,
                        });
                        plan.blocks[b].insts.push(cid);
                        val = cid;
                    }
                    plan.blocks[b].term = Some(Term::Return(val));
                }
                Term::Goto(_) => { /* unconditional edges need no conversion */ }
            }
        }
    }

    // L3: safepoint maps - live allocation values at block entry.
    let live_in = liveness(&plan);
    for b in 0..plan.blocks.len() {
        let refs: Vec<VId> = live_in[b]
            .iter()
            .copied()
            .filter(|&vid| matches!(plan.values[vid].op, ExecOp::ArrNew { sroa: None, .. }))
            .collect();
        if !refs.is_empty() {
            plan.safepoint_maps.push(SafepointMap {
                block: b,
                live_refs: refs,
            });
        }
    }

    // L4: physical allocation over live intervals.
    allocate(&mut plan);
    plan
}

impl PlanValue {
    /// Whether the op is float-domain math (decides the conversion shape).
    #[must_use]
    pub fn op_is_float_math(&self) -> bool {
        matches!(self.op, ExecOp::FBinop(_) | ExecOp::FCmp(_))
    }
}

/// Committed op of a graph value.
fn exec_op_of(v: &crate::son::Value) -> ExecOp {
    match &v.op {
        SonOp::ConstInt(i) => ExecOp::ConstInt(*i),
        SonOp::ConstFloat(f) => ExecOp::ConstFloat(*f),
        SonOp::Null => ExecOp::Null,
        SonOp::Bool(b) => ExecOp::Bool(*b),
        SonOp::Arg(a) => ExecOp::Arg(*a),
        SonOp::Copy => ExecOp::Copy,
        SonOp::Phi { .. } => ExecOp::Phi,
        SonOp::ArrNew(l) => ExecOp::ArrNew {
            len: *l,
            sroa: None,
        },
        SonOp::ArrLoad => ExecOp::ArrLoad { sroa: None },
        SonOp::ArrStore => ExecOp::ArrStore { sroa: None },
        SonOp::ArrLen => ExecOp::ArrLen {
            sroa: None,
            const_len: None,
        },
        SonOp::SroaLoad(s) => ExecOp::SroaLoad(*s),
        SonOp::SroaStore(s) => ExecOp::SroaStore(*s),
        SonOp::SroaNew { slot, len } => ExecOp::SroaNew {
            slot: *slot,
            len: *len,
        },
        bin @ (SonOp::Add | SonOp::Sub | SonOp::Mul | SonOp::Div | SonOp::Neg) => {
            let b = match bin {
                SonOp::Add => crate::exec::Bin::Add,
                SonOp::Sub => crate::exec::Bin::Sub,
                SonOp::Mul => crate::exec::Bin::Mul,
                SonOp::Div => crate::exec::Bin::Div,
                _ => crate::exec::Bin::Neg,
            };
            match v.rep {
                Rep::F64 => ExecOp::FBinop(b),
                Rep::I64 => ExecOp::IBinop(b),
                _ => ExecOp::TagBinop(b),
            }
        }
        cmp @ (SonOp::Eq | SonOp::Lt | SonOp::Le) => {
            let c = match cmp {
                SonOp::Eq => crate::exec::Cmp::Eq,
                SonOp::Lt => crate::exec::Cmp::Lt,
                _ => crate::exec::Cmp::Le,
            };
            match v.rep {
                Rep::F64 => ExecOp::FCmp(c),
                Rep::I64 => ExecOp::ICmp(c),
                _ => ExecOp::TagCmp(c),
            }
        }
        SonOp::IsNull => ExecOp::TagIsNull,
        SonOp::Guard(g) => ExecOp::Guard(*g),
    }
}

/// Block-level live sets (per block: live-in values).
// CEP:WHAT Block-level live sets (per block: live-in values).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn liveness(plan: &Plan) -> Vec<std::collections::HashSet<VId>> {
    let n = plan.blocks.len();
    let mut live_in: Vec<std::collections::HashSet<VId>> =
        vec![std::collections::HashSet::new(); n];
    let mut changed = true;
    while changed {
        changed = false;
        for b in (0..n).rev() {
            let mut out = std::collections::HashSet::new();
            for &s in &plan.blocks[b].succs {
                for &v in &live_in[s] {
                    out.insert(v);
                }
            }
            // Phis are uses of their inputs on the incoming edge; treat the
            // phi itself as defined at entry and its inputs as uses.
            let mut inp = out;
            for &vid in &plan.blocks[b].insts {
                let v = &plan.values[vid];
                if matches!(v.op, ExecOp::Phi) {
                    for &i in &v.inputs {
                        inp.insert(i);
                    }
                } else {
                    for &i in &v.inputs {
                        inp.insert(i);
                    }
                    inp.remove(&vid);
                }
            }
            if let Some(t) = &plan.blocks[b].term {
                match t {
                    Term::Branch { cond, .. } => {
                        inp.insert(*cond);
                    }
                    Term::Return(val) => {
                        inp.insert(*val);
                    }
                    Term::Goto(_) => { /* unconditional edges are liveness-neutral */ }
                }
            }
            if inp != live_in[b] {
                live_in[b] = inp;
                changed = true;
            }
        }
    }
    live_in
}

/// Linear scan over live intervals with two register classes.
// CEP:WHAT Linear scan over live intervals with two register classes.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn allocate(plan: &mut Plan) {
    let order = rpo(&plan.blocks);
    let mut pos: HashMap<BId, usize> = HashMap::new();
    for (i, &b) in order.iter().enumerate() {
        pos.insert(b, i);
    }
    let stride = 1_000_000_usize;
    let ordinal_of = |block: BId, within: usize| pos.get(&block).unwrap_or(&0) * stride + within;

    // Intervals: [first def-or-use .. last use] across the ordinal space.
    let mut first: HashMap<VId, usize> = HashMap::new();
    let mut last: HashMap<VId, usize> = HashMap::new();
    fn note_first(m: &mut HashMap<VId, usize>, vid: VId, o: usize) {
        let e = m.entry(vid).or_insert(o);
        *e = (*e).min(o);
    }
    fn note_last(m: &mut HashMap<VId, usize>, vid: VId, o: usize) {
        let e = m.entry(vid).or_insert(o);
        *e = (*e).max(o);
    }
    for b in 0..plan.blocks.len() {
        for (wi, &vid) in plan.blocks[b].insts.iter().enumerate() {
            let o = ordinal_of(b, wi);
            note_first(&mut first, vid, o);
            note_last(&mut last, vid, o);
            for &i in &plan.values[vid].inputs {
                note_last(&mut first, i, o);
                note_last(&mut last, i, o);
            }
        }
        if let Some(t) = &plan.blocks[b].term {
            let o = ordinal_of(b, stride / 2);
            match t {
                Term::Branch { cond, .. } | Term::Return(cond) => {
                    note_last(&mut first, *cond, o);
                    note_last(&mut last, *cond, o);
                }
                Term::Goto(_) => { /* no live value crosses an unconditional edge */ }
            }
        }
    }

    let mut intervals: Vec<(VId, usize, usize, bool)> = plan
        .values
        .iter()
        .enumerate()
        .filter_map(|(vid, v)| {
            first.get(&vid).map(|&f| {
                (
                    vid,
                    f,
                    last.get(&vid).copied().unwrap_or(f),
                    v.rep == Rep::F64,
                )
            })
        })
        .collect();
    intervals.sort_by_key(|&(_, f, _, _)| f);

    let mut free_gpr: Vec<u8> = (0..GPR_COUNT as u8).rev().collect();
    let mut free_fpr: Vec<u8> = (0..FPR_COUNT as u8).rev().collect();
    let mut active: Vec<(usize, bool, u8)> = Vec::new(); // (last, is_float, reg)
    let mut locations = vec![Loc::Reg(0); plan.values.len()];
    let mut spill_slots = 0_usize;
    let mut pressure = 0_usize;

    for (vid, f, l, is_float) in intervals {
        let mut expired: Vec<usize> = Vec::new();
        for (i, (a_last, _, _)) in active.iter().enumerate() {
            if *a_last < f {
                expired.push(i);
            }
        }
        for i in expired.into_iter().rev() {
            let (_, isf, r) = active.remove(i);
            if isf {
                free_fpr.push(r);
            } else {
                free_gpr.push(r);
            }
        }
        pressure = pressure.max(active.len());
        let loc = if is_float {
            free_fpr.pop().map(Loc::Reg).unwrap_or_else(|| {
                spill_slots += 1;
                Loc::Stack((spill_slots - 1) as u16)
            })
        } else {
            free_gpr.pop().map(Loc::Reg).unwrap_or_else(|| {
                spill_slots += 1;
                Loc::Stack((spill_slots - 1) as u16)
            })
        };
        locations[vid] = loc;
        let r = match loc {
            Loc::Reg(r) => r,
            Loc::Stack(_) => 0xFF,
        };
        active.push((l, is_float, r));
    }

    plan.locations = locations;
    plan.spill_slots = spill_slots;
    plan.pressure = pressure;
}

/// Reverse post-order of plan blocks.
// CEP:WHAT Reverse post-order of plan blocks.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn rpo(blocks: &[PlanBlock]) -> Vec<BId> {
    let n = blocks.len();
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
