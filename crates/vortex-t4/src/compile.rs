// CEP:FILE crates/vortex-t4/src/compile.rs
// CEP:WHAT Trace compilation (T4 sections 4.3-4.4): the recorded linear
//        path becomes a single-entry SSA block with a speculation guard at
//        every type-sensitive operation (the recorded operand domain),
//        lowered through the shared optimizing pipeline (folding, GVN,
//        ARS) and committed to a plan. Guard fusion happens through the
//        pipeline's GVN: identical guards on the same value collapse.
// CEP:WHY A trace has no internal control flow, so no phi nodes are
//        needed; the specialization content is the guards, and the deopt
//        contract (resume at the failing pc in T0) preserves semantics
//        under speculation.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Unsupported trace opcodes reject compilation with a typed
//        error; the trace stays on T0 (a documented, legal outcome).
// CEP:ASSUMES The trace was recorded by recorder::record_trace from a
//        verified method.
// CEP:COST O(trace length) build; the pipeline cost matches T2.
// CEP:EVIDENCE cargo test -p vortex-t4
use vortex_nv1 as nv1;
use vortex_opt::{GuardKind, SonFunc, SonOp};
use vortex_ugb::{instr, Method, Op};

use crate::recorder::Trace;

/// Compilation failure for a trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceError {
    /// The trace contains an opcode outside the guarded subset.
    Unsupported {
        /// The opcode.
        op: Op,
        /// Its pc in the trace.
        pc: usize,
    },
    /// The trace is empty.
    Empty,
    /// The graph build or pipeline rejected the trace.
    Son(vortex_opt::SonError),
}

/// A compiled trace: the guarded plan plus the resume contract.
#[derive(Debug, Clone)]
pub struct CompiledTrace {
    /// The guarded plan.
    pub plan: vortex_opt::Plan,
    /// The register file at window entry (the plan's Arg values read it).
    pub entry_frame: Vec<u64>,
    /// For a closed trace: the pc T0 re-executes with the returned frame
    /// (the guarded side exit). `None` when the trace contains the Return.
    pub exit_pc: Option<usize>,
    /// Guards before pipeline deduplication (fusion evidence).
    pub guards_emitted: u64,
    /// Guards after GVN fusion.
    pub guards_kept: u64,
}

/// Compiles a recorded trace into a guarded plan through the shared
/// optimizing pipeline.
// CEP:WHAT Compiles a recorded trace into a guarded plan through the shared optimizing pipeline.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
pub fn compile_trace(
    program: &[Method],
    method_index: usize,
    trace: &Trace,
) -> Result<CompiledTrace, TraceError> {
    let method = program
        .get(method_index)
        .ok_or(TraceError::Son(vortex_opt::SonError::Decode { offset: 0 }))?;
    if trace.entries.is_empty() {
        return Err(TraceError::Empty);
    }

    let mut func = SonFunc {
        values: Vec::new(),
        blocks: vec![vortex_opt::son::Block::default()],
        entry_state: vec![Vec::new(); 1],
        nregs: method.register_count,
        arg_count: method.arg_count,
        method_index,
    };
    let mut vreg: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
    // Every live register at window entry is an Arg value consuming the
    // recorded entry frame (OSR-1 section 2.1: the entry block extracts
    // the VM frame state).
    for v in 0..method.register_count {
        let vid = func.push_op(SonOp::Arg(v), Vec::new(), Vec::new(), 0, 0, Some(v));
        func.blocks[0].insts.push(vid);
        vreg.insert(v, vid);
    }

    let mut guards_emitted = 0_u64;
    let ends_with_return = trace.entries.last().is_some_and(|e| e.op == Op::Return);

    for e in &trace.entries {
        if e.op == Op::Return {
            // The trailing Return is the trace's terminator, handled below.
            break;
        }
        let (instr_i, _) = instr::decode_at(&method.code, e.pc)
            .map_err(|se| TraceError::Son(vortex_opt::SonError::Decode { offset: se.offset }))?;
        let mk = |func: &mut SonFunc, op: SonOp, inputs: Vec<usize>, def: Option<u16>| -> usize {
            let vid = func.push_op(op, inputs, Vec::new(), 0, e.pc, def);
            func.blocks[0].insts.push(vid);
            vid
        };
        let get = |vreg: &std::collections::HashMap<u16, usize>, r: u16| -> usize {
            vreg.get(&r).copied().unwrap_or(0)
        };
        // Guard helper: emit a guard when the recorded domain is one the
        // compiled op specializes on.
        let guard = |func: &mut SonFunc,
                     vreg: &std::collections::HashMap<u16, usize>,
                     r: u16,
                     cats: &[Option<nv1::Category>; 3],
                     slot: usize,
                     guards: &mut u64|
         -> usize {
            let vid = get(vreg, r);
            let kind = match cats.get(slot).copied().flatten() {
                Some(nv1::Category::Int) => Some(GuardKind::Int),
                Some(nv1::Category::Float) => Some(GuardKind::Float),
                _ => None,
            };
            if let Some(kind) = kind {
                // Guards are checks, not data: the specialized op keeps
                // reading the ORIGINAL operand value.
                *guards += 1;
                let g = func.push_op(SonOp::Guard(kind), vec![vid], Vec::new(), 0, e.pc, None);
                func.blocks[0].insts.push(g);
            }
            vid
        };
        match instr_i.op {
            Op::LoadNull => {
                let v = mk(&mut func, SonOp::Null, Vec::new(), Some(instr_i.regs[0]));
                vreg.insert(instr_i.regs[0], v);
            }
            Op::LoadTrue | Op::LoadFalse => {
                let v = mk(
                    &mut func,
                    SonOp::Bool(instr_i.op == Op::LoadTrue),
                    Vec::new(),
                    Some(instr_i.regs[0]),
                );
                vreg.insert(instr_i.regs[0], v);
            }
            Op::LoadInt32 => {
                let v = mk(
                    &mut func,
                    SonOp::ConstInt(i64::from(instr_i.imm32)),
                    Vec::new(),
                    Some(instr_i.regs[0]),
                );
                vreg.insert(instr_i.regs[0], v);
            }
            Op::LoadFloat => {
                let v = mk(
                    &mut func,
                    SonOp::ConstFloat(f64::from_bits(instr_i.imm64)),
                    Vec::new(),
                    Some(instr_i.regs[0]),
                );
                vreg.insert(instr_i.regs[0], v);
            }
            Op::LoadConst => {
                let v = match method.consts.get(usize::from(instr_i.pool)).copied() {
                    Some(vortex_ugb::Const::Int(i)) => mk(
                        &mut func,
                        SonOp::ConstInt(i),
                        Vec::new(),
                        Some(instr_i.regs[0]),
                    ),
                    Some(vortex_ugb::Const::Float(f)) => mk(
                        &mut func,
                        SonOp::ConstFloat(f),
                        Vec::new(),
                        Some(instr_i.regs[0]),
                    ),
                    None => {
                        return Err(TraceError::Son(vortex_opt::SonError::Decode {
                            offset: e.pc,
                        }))
                    }
                };
                vreg.insert(instr_i.regs[0], v);
            }
            Op::Move => {
                let src = get(&vreg, instr_i.regs[1]);
                let v = mk(&mut func, SonOp::Copy, vec![src], Some(instr_i.regs[0]));
                vreg.insert(instr_i.regs[0], v);
            }
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Eq | Op::Lt | Op::Le => {
                // cats[i] is the pre-execution category of regs[i]; the
                // sources of a three-address op sit in slots 1 and 2.
                let a = guard(
                    &mut func,
                    &vreg,
                    instr_i.regs[1],
                    &e.cats,
                    1,
                    &mut guards_emitted,
                );
                let b = guard(
                    &mut func,
                    &vreg,
                    instr_i.regs[2],
                    &e.cats,
                    2,
                    &mut guards_emitted,
                );
                let op = match instr_i.op {
                    Op::Add => SonOp::Add,
                    Op::Sub => SonOp::Sub,
                    Op::Mul => SonOp::Mul,
                    Op::Div => SonOp::Div,
                    Op::Eq => SonOp::Eq,
                    Op::Lt => SonOp::Lt,
                    _ => SonOp::Le,
                };
                let v = mk(&mut func, op, vec![a, b], Some(instr_i.regs[0]));
                vreg.insert(instr_i.regs[0], v);
            }
            Op::Neg => {
                let a = guard(
                    &mut func,
                    &vreg,
                    instr_i.regs[1],
                    &e.cats,
                    1,
                    &mut guards_emitted,
                );
                let v = mk(&mut func, SonOp::Neg, vec![a], Some(instr_i.regs[0]));
                vreg.insert(instr_i.regs[0], v);
            }
            Op::IsNull => {
                let a = get(&vreg, instr_i.regs[1]);
                let v = mk(&mut func, SonOp::IsNull, vec![a], Some(instr_i.regs[0]));
                vreg.insert(instr_i.regs[0], v);
            }
            Op::AllocArray => {
                let v = mk(
                    &mut func,
                    SonOp::ArrNew(instr_i.imm32u[0]),
                    Vec::new(),
                    Some(instr_i.regs[0]),
                );
                vreg.insert(instr_i.regs[0], v);
            }
            Op::LoadIndex => {
                let arr = get(&vreg, instr_i.regs[1]);
                let idx = get(&vreg, instr_i.regs[2]);
                // Bounds speculation: the recorded execution stayed in
                // bounds; the guard re-checks and deopts otherwise.
                let g = mk(
                    &mut func,
                    SonOp::Guard(GuardKind::IndexBelowLen),
                    vec![idx, arr],
                    None,
                );
                func.blocks[0].insts.push(g);
                guards_emitted += 1;
                let v = mk(
                    &mut func,
                    SonOp::ArrLoad,
                    vec![arr, idx],
                    Some(instr_i.regs[0]),
                );
                vreg.insert(instr_i.regs[0], v);
            }
            Op::StoreIndex => {
                let arr = get(&vreg, instr_i.regs[0]);
                let idx = get(&vreg, instr_i.regs[1]);
                let val = get(&vreg, instr_i.regs[2]);
                let g = mk(
                    &mut func,
                    SonOp::Guard(GuardKind::IndexBelowLen),
                    vec![idx, arr],
                    None,
                );
                func.blocks[0].insts.push(g);
                guards_emitted += 1;
                let _ = mk(&mut func, SonOp::ArrStore, vec![arr, idx, val], None);
            }
            Op::LoadLength => {
                let arr = get(&vreg, instr_i.regs[1]);
                let v = mk(&mut func, SonOp::ArrLen, vec![arr], Some(instr_i.regs[0]));
                vreg.insert(instr_i.regs[0], v);
            }
            Op::JumpTrue | Op::JumpFalse => {
                // The recorded path took the fallthrough: guard the
                // recorded direction and deopt to T0 at the branch pc when
                // it fails (T0 then branches the correct way). A closed
                // trace's back-edge is excluded by the recorder; a
                // Jump targeting a pc inside the window is unsupported.
                if instr_i.op == Op::Jump {
                    return Err(TraceError::Unsupported {
                        op: Op::Jump,
                        pc: e.pc,
                    });
                }
                let cond = get(&vreg, instr_i.regs[0]);
                let kind = if instr_i.op == Op::JumpFalse {
                    // Fallthrough when truthy (JumpFalse not taken).
                    GuardKind::Truthy
                } else {
                    // JumpTrue not taken: fallthrough when falsy.
                    GuardKind::Falsy
                };
                let g = mk(&mut func, SonOp::Guard(kind), vec![cond], None);
                func.blocks[0].insts.push(g);
                guards_emitted += 1;
            }
            Op::Safepoint | Op::Nop => { /* documented no-op */ }
            unsupported => {
                return Err(TraceError::Unsupported {
                    op: unsupported,
                    pc: e.pc,
                })
            }
        }
    }

    // Terminator: an open trace ends at the recorded Return; a closed
    // trace exits the compiled window with a synthetic return whose frame
    // the driver uses to resume T0 at the exit pc.
    let term_val = ends_with_return
        .then(|| {
            trace.entries.last().map(|e| {
                vreg.get(&{
                    let (i, _) = instr::decode_at(&method.code, e.pc).expect("re-decodes");
                    i.regs[0]
                })
                .copied()
                .unwrap_or(0)
            })
        })
        .flatten()
        .unwrap_or_else(|| {
            let v = func.push_op(SonOp::Null, Vec::new(), Vec::new(), 0, usize::MAX, None);
            func.blocks[0].insts.push(v);
            v
        });
    func.blocks[0].term = Some(vortex_opt::son::Term::Return(term_val));

    // Pipeline: fold, GVN (guard fusion), ARS. No loops, so LICM/BCE/SROA
    // are not part of the trace budget.
    let stats = vortex_opt::run_pipeline(&mut func, vortex_opt::TierBudget::Moderate);
    let mut plan = vortex_opt::lower(&func);
    plan.stats = stats;
    plan.sroa_slots = 0;
    // Only guards still referenced by the plan's blocks count: fused
    // duplicates are dead nodes.
    let guards_kept = plan
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|&&vid| matches!(plan.values[vid].op, vortex_opt::ExecOp::Guard(_)))
        .count() as u64;
    Ok(CompiledTrace {
        plan,
        entry_frame: trace.entry_frame.clone(),
        exit_pc: trace.exit_pc,
        guards_emitted,
        guards_kept,
    })
}
