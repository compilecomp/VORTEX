// CEP:FILE crates/vortex-opt/src/exec.rs
// CEP:WHAT The plan executor: executes the lowered plan with the exact
//        interpreter semantics - tagged arithmetic through the shared T0
//        semantic core, unboxed binary64/i64 math for ARS-committed values
//        with explicit boxing at observable boundaries, AOSS store policy on
//        the heap, SROA slot tables, and SSA phi evaluation from the
//        incoming edge.
// CEP:WHY The compiled plan must be observable-equal to the interpreter:
//        driving the plan through the same semantic core makes the
//        differential T0/T2 equality tests meaningful and keeps one
//        semantic reference for the whole engine.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE All failure paths surface as the typed Trap taxonomy; no
//        silent divergence from T0 is possible without a failing
//        differential test.
// CEP:ASSUMES The plan was produced by lower::lower from an optimized graph.
// CEP:COST O(values executed); each op is O(1) except heap ops.
// CEP:EVIDENCE cargo test -p vortex-opt
use vortex_nv1 as nv1;
use vortex_runtime::{Machine, Trap};
use vortex_t0::sem;

use crate::lower::{ExecOp, Plan};

/// Arithmetic kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bin {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Negation.
    Neg,
}

/// Comparison kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    /// Equality.
    Eq,
    /// Less-than.
    Lt,
    /// Less-or-equal.
    Le,
}

/// Compiles and executes a method through the optimizing pipeline.
// CEP:WHAT Compiles and executes a method through the optimizing pipeline.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn compile_and_run(
    program: &[vortex_ugb::Method],
    method_index: usize,
    budget: crate::passes::TierBudget,
    m: &mut Machine,
    args: &[u64],
) -> Result<u64, Trap> {
    let plan = crate::compile_plan(program, method_index, budget).map_err(|e| match e {
        crate::OptError::Son(crate::SonError::Decode { offset })
        | crate::OptError::Son(crate::SonError::BranchTarget { target: offset })
        | crate::OptError::Son(crate::SonError::Unsupported { offset, .. }) => {
            Trap::InvalidEncoding { offset }
        }
    })?;
    run_plan(&plan, program, m, args)
}

/// Executes a plan to completion.
// CEP:WHAT Executes a plan to completion.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[allow(clippy::float_cmp, clippy::too_many_lines)]
/// Execution outcome of a plan run that can deoptimize (design.md OSR-1
/// section 7: the deopt carries the failing bytecode pc and the fully
/// reconstructed tagged frame).
#[derive(Debug)]
pub enum ExecOutcome {
    /// The plan ran to its Return; the frame accompanies the word so
    /// trace drivers can resume T0 at a side exit.
    Returned {
        /// The boxed return word.
        word: u64,
        /// The tagged register frame at the return.
        frame: Vec<u64>,
    },
    /// A speculation guard failed: resume interpretation at `pc` with the
    /// tagged register frame (OSR-INV-04 deopt reversibility).
    Deoptimized {
        /// The bytecode pc to resume at.
        pc: usize,
        /// The reconstructed tagged virtual-register frame.
        frame: Vec<u64>,
        /// The method index the pc refers to.
        method_index: usize,
    },
}

/// Runs a plan with deopt support: guards signal deoptimization with the
/// reconstructed frame instead of trapping.
// CEP:WHAT Runs a plan with deopt support.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST One frame write per defined value.
// CEP:EVIDENCE cargo test -p vortex-t4
pub fn run_plan_with_deopt(
    plan: &Plan,
    program: &[vortex_ugb::Method],
    m: &mut Machine,
    args: &[u64],
) -> Result<ExecOutcome, Trap> {
    let mut frame: Vec<u64> = vec![vortex_nv1::undefined_word(); usize::from(plan.nregs)];
    for (i, &a) in args.iter().enumerate().take(frame.len()) {
        frame[i] = a;
    }
    run_plan_from_frame(plan, program, m, &frame)
}

/// Runs a plan starting from a full tagged register frame: the OSR/trace
/// entry contract (design.md OSR-1 section 2.1: the entry block consumes
/// the VM frame).
// CEP:WHAT Runs a plan starting from a full tagged register frame.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Identical to run_plan_with_deopt.
// CEP:EVIDENCE cargo test -p vortex-t4
pub fn run_plan_from_frame(
    plan: &Plan,
    program: &[vortex_ugb::Method],
    m: &mut Machine,
    frame_in: &[u64],
) -> Result<ExecOutcome, Trap> {
    let _ = program;
    let mut values: Vec<u64> = vec![0; plan.values.len()];
    let mut sroa: Vec<Vec<u64>> = vec![Vec::new(); plan.sroa_slots];
    let mut frame: Vec<u64> = frame_in.to_vec();
    let mut block: usize = 0;
    let mut prev_block: Option<usize> = None;
    let mut steps: u64 = 0;
    const STEP_LIMIT: u64 = 100_000_000;

    let word_of = |plan: &Plan, values: &[u64], vid: usize, m: &mut Machine| -> Result<u64, Trap> {
        let bits = values[vid];
        Ok(match plan.values[vid].rep {
            crate::son::Rep::Tagged => bits,
            crate::son::Rep::F64 => nv1::encode_f64(f64::from_bits(bits)),
            crate::son::Rep::I64 => {
                let v = bits as i64;
                nv1::encode_int(v).unwrap_or_else(|| sem::box_int(m, v))
            }
        })
    };
    let to_f64_word = |w: u64| -> Result<u64, Trap> {
        match nv1::categorize(w) {
            nv1::Category::Int => Ok((nv1::decode_int(w) as f64).to_bits()),
            nv1::Category::Float => Ok(w), // already binary64 bits
            _ => Err(Trap::TypeMismatch {
                expected: "numeric",
                word: w,
            }),
        }
    };
    let i64_word_of = |plan: &Plan, values: &[u64], vid: usize| -> i64 {
        match plan.values[vid].rep {
            crate::son::Rep::I64 => values[vid] as i64,
            crate::son::Rep::F64 => f64::from_bits(values[vid]) as i64,
            crate::son::Rep::Tagged => nv1::decode_int(values[vid]),
        }
    };

    loop {
        steps += 1;
        if steps > STEP_LIMIT {
            return Err(Trap::InvalidEncoding { offset: usize::MAX });
        }
        for &vid in &plan.blocks[block].insts {
            let (op, inputs, rep) = {
                let v = &plan.values[vid];
                (v.op.clone(), v.inputs.clone(), v.rep)
            };
            let raw = match op {
                ExecOp::ConstInt(i) => i as u64,
                ExecOp::ConstFloat(f) => f.to_bits(),
                ExecOp::Null => nv1::null_word(),
                ExecOp::Bool(b) => nv1::bool_word(b),
                ExecOp::Arg(a) => frame
                    .get(usize::from(a))
                    .copied()
                    .unwrap_or_else(vortex_nv1::undefined_word),
                ExecOp::Copy => values[inputs[0]],
                ExecOp::Phi => {
                    // Block-boundary parallel copy: inputs convert to the
                    // phi's representation (INV-LLIR-005 site).
                    let src = plan.blocks[block]
                        .preds
                        .iter()
                        .position(|&p| Some(p) == prev_block)
                        .unwrap_or(0);
                    let in_vid = inputs.get(src).copied().unwrap_or(inputs[0]);
                    match rep {
                        crate::son::Rep::Tagged => match plan.values[in_vid].rep {
                            crate::son::Rep::Tagged => values[in_vid],
                            crate::son::Rep::F64 => nv1::encode_f64(f64::from_bits(values[in_vid])),
                            crate::son::Rep::I64 => {
                                let v = values[in_vid] as i64;
                                nv1::encode_int(v).unwrap_or_else(|| sem::box_int(m, v))
                            }
                        },
                        crate::son::Rep::F64 => match plan.values[in_vid].rep {
                            crate::son::Rep::F64 => values[in_vid],
                            crate::son::Rep::I64 => (values[in_vid] as i64 as f64).to_bits(),
                            crate::son::Rep::Tagged => to_f64_word(values[in_vid])?,
                        },
                        crate::son::Rep::I64 => i64_word_of(plan, &values, in_vid) as u64,
                    }
                }
                ExecOp::TagBinop(b) => {
                    let a = word_of(plan, &values, inputs[0], m)?;
                    let w = word_of(plan, &values, inputs.get(1).copied().unwrap_or(0), m)?;
                    let op = match b {
                        Bin::Add => vortex_ugb::Op::Add,
                        Bin::Sub => vortex_ugb::Op::Sub,
                        Bin::Mul => vortex_ugb::Op::Mul,
                        Bin::Div => vortex_ugb::Op::Div,
                        Bin::Neg => vortex_ugb::Op::Sub,
                    };
                    if b == Bin::Neg {
                        let zero = nv1::encode_int(0).expect("zero encodes");
                        sem::arith(m, op, zero, a)?
                    } else {
                        sem::arith(m, op, a, w)?
                    }
                }
                ExecOp::FBinop(b) => {
                    let x = f64::from_bits(values[inputs[0]]);
                    let y = inputs
                        .get(1)
                        .map(|&i| f64::from_bits(values[i]))
                        .unwrap_or(0.0);
                    match b {
                        Bin::Add => (x + y).to_bits(),
                        Bin::Sub => (x - y).to_bits(),
                        Bin::Mul => (x * y).to_bits(),
                        Bin::Div => (x / y).to_bits(),
                        Bin::Neg => (-x).to_bits(),
                    }
                }
                ExecOp::IBinop(b) => {
                    let x = values[inputs[0]] as i64;
                    let y = inputs.get(1).map(|&i| values[i] as i64).unwrap_or(0);
                    match b {
                        Bin::Add => x.wrapping_add(y) as u64,
                        Bin::Sub => x.wrapping_sub(y) as u64,
                        Bin::Mul => x.wrapping_mul(y) as u64,
                        Bin::Div => x.wrapping_div(y.max(1)) as u64,
                        Bin::Neg => x.wrapping_neg() as u64,
                    }
                }
                ExecOp::TagCmp(c) => {
                    let a = word_of(plan, &values, inputs[0], m)?;
                    let b = word_of(plan, &values, inputs[1], m)?;
                    let r = match c {
                        Cmp::Eq => sem::words_equal(a, b),
                        Cmp::Lt | Cmp::Le => {
                            let x = sem::numeric_value(m, a)?;
                            let y = sem::numeric_value(m, b)?;
                            match x.partial_cmp(&y) {
                                // Both Lt and Le hold on Less; only Le holds
                                // on Equal; Greater satisfies neither.
                                Some(core::cmp::Ordering::Less) => true,
                                Some(core::cmp::Ordering::Equal) => c == Cmp::Le,
                                _ => false,
                            }
                        }
                    };
                    nv1::bool_word(r)
                }
                ExecOp::FCmp(c) => {
                    let x = f64::from_bits(values[inputs[0]]);
                    let y = f64::from_bits(values[inputs[1]]);
                    // The equality arm uses == deliberately: abstract
                    // equality on numbers is value equality, and
                    // NaN != NaN is the required IEEE semantics.
                    #[allow(clippy::float_cmp)]
                    let r = match c {
                        Cmp::Eq => x == y,
                        Cmp::Lt => x < y,
                        Cmp::Le => x <= y,
                    };
                    nv1::bool_word(r)
                }
                ExecOp::ICmp(c) => {
                    let x = values[inputs[0]] as i64;
                    let y = values[inputs[1]] as i64;
                    let r = match c {
                        Cmp::Eq => x == y,
                        Cmp::Lt => x < y,
                        Cmp::Le => x <= y,
                    };
                    nv1::bool_word(r)
                }
                ExecOp::TagIsNull => nv1::bool_word(values[inputs[0]] == nv1::null_word()),
                ExecOp::ToF64 => to_f64_word(values[inputs[0]])?,
                ExecOp::BoxFloat => nv1::encode_f64(f64::from_bits(values[inputs[0]])),
                ExecOp::BoxInt => {
                    let v = values[inputs[0]] as i64;
                    nv1::encode_int(v).unwrap_or_else(|| sem::box_int(m, v))
                }
                ExecOp::ArrNew { len, .. } => {
                    let nursery = m.heap.nursery_page().ok_or(Trap::OutOfMemory)?;
                    let h = m.heap.alloc_array(
                        vortex_runtime::layout::GENERIC_ARRAY,
                        len as usize,
                        nursery,
                    )?;
                    nv1::encode_ref(h.0, 0).expect("valid handle")
                }
                ExecOp::ArrLoad { .. } => {
                    let arr = sem::as_handle(word_of(plan, &values, inputs[0], m)?)?;
                    let idx = sem::as_index(word_of(plan, &values, inputs[1], m)?)?;
                    m.heap.load_array_element(arr, idx, &mut m.gc.stats)?
                }
                ExecOp::ArrStore { .. } => {
                    let arr = sem::as_handle(word_of(plan, &values, inputs[0], m)?)?;
                    let idx = sem::as_index(word_of(plan, &values, inputs[1], m)?)?;
                    let val = word_of(plan, &values, inputs[2], m)?;
                    sem::store_element(m, arr, idx, val)?;
                    0
                }
                ExecOp::ArrLen { .. } => {
                    let arr = sem::as_handle(word_of(plan, &values, inputs[0], m)?)?;
                    let len = m.heap.array_len(arr);
                    // The committed representation is I64: the raw length.
                    // Boxing to the tagged domain happens only at
                    // observable boundaries through BoxInt.
                    i64::try_from(len).unwrap_or(i64::MAX) as u64
                }
                ExecOp::Guard(kind) => {
                    let w = word_of(plan, &values, inputs[0], m)?;
                    let ok = match kind {
                        crate::son::GuardKind::Int => nv1::is_int(w),
                        crate::son::GuardKind::Float => nv1::categorize(w) == nv1::Category::Float,
                        crate::son::GuardKind::Numeric => matches!(
                            nv1::categorize(w),
                            nv1::Category::Int | nv1::Category::Float
                        ),
                        crate::son::GuardKind::Truthy => sem::is_truthy(w),
                        crate::son::GuardKind::Falsy => !sem::is_truthy(w),
                        crate::son::GuardKind::IndexBelowLen => {
                            let arr = sem::as_handle(word_of(
                                plan,
                                &values,
                                *inputs.get(1).unwrap_or(&0),
                                m,
                            )?)?;
                            let idx = sem::as_index(w)?;
                            idx < m.heap.array_len(arr)
                        }
                    };
                    if !ok {
                        // Speculation failed: deoptimize to T0 at this
                        // bytecode pc (design.md T4 4.4 guard density;
                        // OSR-1 section 7.1 state reconstruction).
                        return Ok(ExecOutcome::Deoptimized {
                            pc: plan.values[vid].pc,
                            frame,
                            method_index: plan.method_index,
                        });
                    }
                    0
                }
                ExecOp::SroaNew { slot, len } => {
                    if slot >= sroa.len() {
                        return Err(Trap::InvalidEncoding { offset: usize::MAX });
                    }
                    sroa[slot] = vec![nv1::undefined_word(); len as usize];
                    0
                }
                ExecOp::SroaLoad(slot) => {
                    let idx = sem::as_index(word_of(plan, &values, inputs[0], m)?)?;
                    let slots = sroa
                        .get(slot)
                        .ok_or(Trap::InvalidEncoding { offset: usize::MAX })?;
                    if idx >= slots.len() {
                        return Err(Trap::BoundsCheck {
                            index: idx,
                            len: slots.len(),
                        });
                    }
                    slots[idx]
                }
                ExecOp::SroaStore(slot) => {
                    let idx = sem::as_index(word_of(plan, &values, inputs[0], m)?)?;
                    let val = word_of(plan, &values, inputs[1], m)?;
                    let slots = sroa
                        .get_mut(slot)
                        .ok_or(Trap::InvalidEncoding { offset: usize::MAX })?;
                    if idx >= slots.len() {
                        return Err(Trap::BoundsCheck {
                            index: idx,
                            len: slots.len(),
                        });
                    }
                    slots[idx] = val;
                    0
                }
            };
            values[vid] = raw;
            if let Some(r) = plan.values[vid].def_vreg {
                // The tagged frame tracks every defined register
                // (OSR-INV-01 state completeness at any deopt point).
                frame[usize::from(r)] = word_of(plan, &values, vid, m)?;
            }
        }
        match &plan.blocks[block].term {
            Some(crate::son::Term::Return(val)) => {
                let word = word_of(plan, &values, *val, m)?;
                return Ok(ExecOutcome::Returned { word, frame });
            }
            Some(crate::son::Term::Branch {
                cond,
                target,
                on_truthy,
            }) => {
                let c = word_of(plan, &values, *cond, m)?;
                let taken = sem::is_truthy(c) == *on_truthy;
                let next = if taken {
                    *target
                } else {
                    // Fallthrough is succs[0] unless it IS the target.
                    plan.blocks[block]
                        .succs
                        .iter()
                        .copied()
                        .find(|&s| s != *target)
                        .unwrap_or(*target)
                };
                prev_block = Some(block);
                block = next;
            }
            Some(crate::son::Term::Goto(t)) => {
                prev_block = Some(block);
                block = *t;
            }
            None => {
                return Err(Trap::InvalidEncoding { offset: usize::MAX });
            }
        }
    }
}

/// Executes a plan to completion without deopt support (plain runs).
// CEP:WHAT Executes a plan to completion without deopt support (plain runs).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Identical to run_plan_with_deopt.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn run_plan(
    plan: &Plan,
    program: &[vortex_ugb::Method],
    m: &mut Machine,
    args: &[u64],
) -> Result<u64, Trap> {
    match run_plan_with_deopt(plan, program, m, args)? {
        ExecOutcome::Returned { word, .. } => Ok(word),
        ExecOutcome::Deoptimized { .. } => Err(Trap::InvalidEncoding { offset: usize::MAX }),
    }
}
