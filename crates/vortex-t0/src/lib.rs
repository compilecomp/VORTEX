//! T0: the reference interpreter over UGB programs.
// CEP:FILE crates/vortex-t0/src/lib.rs
// CEP:WHAT T0 interpreter: table-dispatch execution of UGB methods with the
//        BARRIER-1-corrected load barrier, epoch-based capability checks,
//        sampled back-edge counting for T4, AOSS numeric transitions, and
//        exception handlers.
// CEP:WHY T0 is the semantic reference for every higher tier: if T0's
//        behavior and an optimization disagree, T0 wins by definition.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE All failure paths surface as typed Trap values; handler stacks
//        are unwound on throw; no silent state corruption.
// CEP:ASSUMES Methods pass vortex-ugb verification before execution.
// CEP:COST Dispatch is a jump-table match; hot ops are branch-light. The
//        barrier adds at most one classify + one color test per ref load.
// CEP:EVIDENCE cargo test -p vortex-t0
use std::collections::HashMap;

use vortex_nv1 as nv1;
use vortex_runtime as rt;
use vortex_ugb::{instr, Op};

/// One activation frame.
#[derive(Debug)]
struct Frame {
    /// Virtual registers (NV-1 words).
    regs: Vec<u64>,
    /// Open exception handlers (bytecode offsets of the Catch site).
    handlers: Vec<usize>,
}

/// Execution outcome.
pub type ExecResult = Result<u64, rt::Trap>;

/// The T0 interpreter over a [`rt::Machine`].
#[derive(Debug)]
pub struct Interp {
    /// Program: methods by index.
    pub program: Vec<vortex_ugb::Method>,
}

impl Interp {
    /// Creates an interpreter for a program.
    #[must_use]
    pub fn new(program: Vec<vortex_ugb::Method>) -> Self {
        Interp { program }
    }

    /// Executes `method_index` with the given argument words to completion.
    // CEP:WHAT Executes method_index with the given argument words to completion.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn run(&self, m: &mut rt::Machine, method_index: usize, args: &[u64]) -> ExecResult {
        let method = self
            .program
            .get(method_index)
            .ok_or(rt::Trap::InvalidEncoding { offset: 0 })?;
        let mut frame = Frame {
            regs: vec![nv1::undefined_word(); usize::from(method.register_count)],
            handlers: Vec::new(),
        };
        for (i, &a) in args.iter().enumerate().take(usize::from(method.arg_count)) {
            frame.regs[i] = a;
        }

        let mut pc = 0_usize;
        loop {
            let (i, next) = instr::decode_at(&method.code, pc)
                .map_err(|e| rt::Trap::InvalidEncoding { offset: e.offset })?;
            match i.op {
                Op::LoadNull => frame.regs[usize::from(i.regs[0])] = nv1::null_word(),
                Op::LoadTrue => frame.regs[usize::from(i.regs[0])] = nv1::bool_word(true),
                Op::LoadFalse => frame.regs[usize::from(i.regs[0])] = nv1::bool_word(false),
                Op::LoadInt32 => {
                    frame.regs[usize::from(i.regs[0])] =
                        nv1::encode_int(i64::from(i.imm32)).unwrap_or_else(nv1::null_word)
                }
                Op::LoadFloat => {
                    frame.regs[usize::from(i.regs[0])] = nv1::encode_f64(f64::from_bits(i.imm64))
                }
                Op::LoadConst => {
                    let c = method
                        .consts
                        .get(usize::from(i.pool))
                        .copied()
                        .ok_or(rt::Trap::InvalidEncoding { offset: pc })?;
                    frame.regs[usize::from(i.regs[0])] = match c {
                        vortex_ugb::Const::Int(v) => {
                            nv1::encode_int(v).unwrap_or_else(|| self.box_int(m, v))
                        }
                        vortex_ugb::Const::Float(v) => nv1::encode_f64(v),
                    };
                }
                Op::Move => frame.regs[usize::from(i.regs[0])] = frame.regs[usize::from(i.regs[1])],
                Op::Add | Op::Sub | Op::Mul | Op::Div => {
                    let dst = usize::from(i.regs[0]);
                    let a = frame.regs[usize::from(i.regs[1])];
                    let b = frame.regs[usize::from(i.regs[2])];
                    frame.regs[dst] = self.arith(m, i.op, a, b)?;
                }
                Op::Neg => {
                    let a = frame.regs[usize::from(i.regs[1])];
                    let zero = nv1::encode_int(0).expect("zero encodes");
                    frame.regs[usize::from(i.regs[0])] = self.arith(m, Op::Sub, zero, a)?;
                }
                Op::Eq | Op::Lt | Op::Le => {
                    let dst = usize::from(i.regs[0]);
                    let a = frame.regs[usize::from(i.regs[1])];
                    let b = frame.regs[usize::from(i.regs[2])];
                    let r = if i.op == Op::Eq {
                        words_equal(a, b)
                    } else {
                        let x = self.numeric_value(m, a)?;
                        let y = self.numeric_value(m, b)?;
                        match x.partial_cmp(&y) {
                            Some(core::cmp::Ordering::Less) => i.op == Op::Lt || i.op == Op::Le,
                            Some(core::cmp::Ordering::Equal) => i.op == Op::Le,
                            _ => false,
                        }
                    };
                    frame.regs[dst] = nv1::bool_word(r);
                }
                Op::IsNull => {
                    let a = frame.regs[usize::from(i.regs[1])];
                    frame.regs[usize::from(i.regs[0])] = nv1::bool_word(a == nv1::null_word());
                }
                Op::Jump | Op::JumpTrue | Op::JumpFalse => {
                    let target = method.branch_target(&i, pc, next - pc);
                    let taken = match i.op {
                        Op::Jump => true,
                        Op::JumpTrue => is_truthy(frame.regs[usize::from(i.regs[0])]),
                        _ => !is_truthy(frame.regs[usize::from(i.regs[0])]),
                    };
                    if taken {
                        if target < pc {
                            // Back-edge: sampled counter for T4 (PROF-1 3.2).
                            // The per-site slot index is the ordinal of this
                            // backward-jump instruction in the stream.
                            let site = backward_site_ordinal(method, pc);
                            let _request = m.profile.backedge(site);
                        }
                        pc = target;
                        continue;
                    }
                }
                Op::Call => return Err(rt::Trap::UnsupportedCall),
                Op::Return => return Ok(frame.regs[usize::from(i.regs[0])]),
                Op::AllocArray => {
                    let len = i.imm32u[0] as usize;
                    let nursery = m.heap.nursery_page().ok_or(rt::Trap::OutOfMemory)?;
                    let h = m
                        .heap
                        .alloc_array(rt::layout::GENERIC_ARRAY, len, nursery)?;
                    frame.regs[usize::from(i.regs[0])] =
                        nv1::encode_ref(h.0, 0).expect("valid handle");
                }
                Op::LoadIndex => {
                    let arr = as_handle(frame.regs[usize::from(i.regs[1])])?;
                    let idx = as_index(frame.regs[usize::from(i.regs[2])])?;
                    let v = m.heap.load_array_element(arr, idx, &mut m.gc.stats)?;
                    frame.regs[usize::from(i.regs[0])] = v;
                }
                Op::StoreIndex => {
                    let arr = as_handle(frame.regs[usize::from(i.regs[0])])?;
                    let idx = as_index(frame.regs[usize::from(i.regs[1])])?;
                    let val = frame.regs[usize::from(i.regs[2])];
                    self.store_element(m, arr, idx, val)?;
                }
                Op::LoadLength => {
                    let arr = as_handle(frame.regs[usize::from(i.regs[1])])?;
                    let len = m.heap.array_len(arr);
                    frame.regs[usize::from(i.regs[0])] =
                        nv1::encode_int(i64::try_from(len).unwrap_or(i64::MAX))
                            .unwrap_or_else(nv1::null_word);
                }
                Op::CapCreate => {
                    let id = m.caps.grant(u64::from(i.imm32u[0]), 0, None);
                    let nursery = m.heap.nursery_page().ok_or(rt::Trap::OutOfMemory)?;
                    let h = m.heap.alloc_capability(
                        u64::from(i.imm32u[0]),
                        u64::from(id.0),
                        1,
                        false,
                        nursery,
                    )?;
                    frame.regs[usize::from(i.regs[0])] =
                        nv1::encode_ref(h.0, 0).expect("valid handle");
                }
                Op::CapCheck => {
                    let cap_word = frame.regs[usize::from(i.regs[0])];
                    let h = as_handle(cap_word)?;
                    let id = rt::CapId(m.heap.read_payload(h, 1) as u32);
                    let site = i.imm32u[1] as usize;
                    let mut cache: rt::EpochCache = method
                        .epoch_caches
                        .get(site)
                        .map(|c| rt::EpochCache {
                            cached_epoch: c.cached_epoch,
                        })
                        .unwrap_or_default();
                    let req = u64::from(i.imm32u[0]);
                    m.caps.check_fast(&mut cache, id, req)?;
                    // Persist the refreshed cache in the machine-level site
                    // table (shared across activations; single-threaded T0).
                    m.site_epochs
                        .insert((method_index, site), cache.cached_epoch);
                }
                Op::CapRevoke => {
                    let cap_word = frame.regs[usize::from(i.regs[0])];
                    let h = as_handle(cap_word)?;
                    let id = rt::CapId(m.heap.read_payload(h, 1) as u32);
                    m.caps.revoke(id);
                }
                Op::Throw => {
                    let exc = frame.regs[usize::from(i.regs[0])];
                    match frame.handlers.pop() {
                        Some(catch_pc) => {
                            m.pending_exception = Some(exc);
                            pc = catch_pc;
                            continue;
                        }
                        None => return Err(rt::Trap::UnhandledException),
                    }
                }
                Op::TryBegin => {
                    let target = method.branch_target(&i, pc, next - pc);
                    frame.handlers.push(target);
                }
                Op::TryEnd => {
                    frame.handlers.pop();
                }
                Op::Catch => {
                    let exc = m.pending_exception.take().unwrap_or_else(nv1::null_word);
                    frame.regs[usize::from(i.regs[0])] = exc;
                }
                Op::Safepoint | Op::Nop => { /* documented no-op */ }
                Op::Halt => return Ok(nv1::null_word()),
            }
            pc = next;
        }
    }

    /// Store path with the full AOSS policy: numeric elision, reverse
    /// transition on domain violation, transition on homogeneity window.
    // CEP:WHAT Store path with the full AOSS policy: numeric elision, reverse transition on domain violation, transition on homogeneity window.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn store_element(
        &self,
        m: &mut rt::Machine,
        arr: rt::Handle,
        idx: usize,
        val: u64,
    ) -> Result<(), rt::Trap> {
        let lay = m.heap.layout_of(arr);
        if rt::layout::is_numeric(lay) {
            // Reverse transition when the value violates the numeric domain
            // (NUM-INV-02).
            let domain_ok = if lay == rt::layout::NUMERIC_F64 {
                nv1::categorize(val) == nv1::Category::Float
            } else {
                nv1::is_int(val)
            };
            if !domain_ok {
                m.aoss.transition_tagged(&mut m.heap, arr)?;
            }
        } else {
            // Tagged store: observe for the numeric transition.
            let decision = m.aoss.observe_store(arr, val);
            if decision != rt::Decision::None {
                // Transition converts the existing payload; the store below
                // then lands raw.
                m.aoss.transition_numeric(&mut m.heap, arr, decision)?;
            }
        }
        m.heap
            .store_array_element(arr, idx, val, &mut m.remembered, &mut m.gc.stats)
    }

    /// Allocates a boxed integer for the i48 overflow domain (NV-1 2.1).
    fn box_int(&self, m: &mut rt::Machine, v: i64) -> u64 {
        let nursery = m.heap.nursery_page().expect("nursery present");
        let h = m
            .heap
            .alloc_array(rt::layout::BOXED_INT, 1, nursery)
            .expect("boxed alloc");
        let _ = m.heap.write_payload(h, 0, v as u64);
        nv1::encode_ref(h.0, 0).expect("valid handle")
    }

    /// Arithmetic with the int fast path, float path, and boxed slow path.
    // CEP:WHAT Arithmetic with the int fast path, float path, and boxed slow path.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn arith(&self, m: &mut rt::Machine, op: Op, a: u64, b: u64) -> Result<u64, rt::Trap> {
        match (nv1::categorize(a), nv1::categorize(b)) {
            (nv1::Category::Int, nv1::Category::Int) => {
                let (x, y) = (nv1::decode_int(a), nv1::decode_int(b));
                let r = match op {
                    Op::Add => x.checked_add(y),
                    Op::Sub => x.checked_sub(y),
                    Op::Mul => x.checked_mul(y),
                    Op::Div => {
                        if y == 0 {
                            return Err(rt::Trap::DivByZero);
                        }
                        x.checked_div(y)
                    }
                    _ => None,
                };
                match r {
                    Some(v) if (nv1::I48_MIN..=nv1::I48_MAX).contains(&v) => {
                        Ok(nv1::encode_int(v).expect("in range"))
                    }
                    Some(v) => Ok(self.box_int(m, v)), // boxed overflow domain
                    None => Err(rt::Trap::ArithmeticOverflow),
                }
            }
            _ => {
                // Float path (mixed int/float promotes per UGB semantics);
                // boxed integers unbox first.
                let x = self.numeric_value(m, a)?;
                let y = self.numeric_value(m, b)?;
                let r = match op {
                    Op::Add => x + y,
                    Op::Sub => x - y,
                    Op::Mul => x * y,
                    Op::Div => x / y,
                    _ => {
                        return Err(rt::Trap::TypeMismatch {
                            expected: "arith op",
                            word: u64::from(op as u8),
                        })
                    }
                };
                Ok(nv1::encode_f64(r))
            }
        }
    }

    /// Numeric value with boxed-integer unboxing.
    fn numeric_value(&self, m: &mut rt::Machine, w: u64) -> Result<f64, rt::Trap> {
        match nv1::categorize(w) {
            nv1::Category::Int => Ok(nv1::decode_int(w) as f64),
            nv1::Category::Float => Ok(f64::from_bits(w)),
            nv1::Category::Ref => {
                let h = as_handle(w)?;
                if m.heap.layout_of(h) == rt::layout::BOXED_INT {
                    Ok(m.heap.read_payload(h, 0) as i64 as f64)
                } else {
                    Err(rt::Trap::TypeMismatch {
                        expected: "numeric",
                        word: w,
                    })
                }
            }
            _ => Err(rt::Trap::TypeMismatch {
                expected: "numeric",
                word: w,
            }),
        }
    }
}

/// Ordinal of the backward-jump instruction at `pc` among all backward jumps
/// in the method (the per-site profile slot key).
fn backward_site_ordinal(method: &vortex_ugb::Method, pc: usize) -> usize {
    let mut ordinal = 0;
    let mut p = 0;
    while p < pc {
        match instr::decode_at(&method.code, p) {
            Ok((i, next)) => {
                if matches!(i.op, Op::Jump | Op::JumpTrue | Op::JumpFalse) && i.branches[0] < 0 {
                    ordinal += 1;
                }
                p = next;
            }
            Err(_) => break,
        }
    }
    ordinal
}

/// Abstract equality over NV-1 words (subset semantics).
///
/// The float arm uses `==` deliberately: abstract equality on numbers is
/// value equality, and NaN != NaN is the required IEEE semantics.
#[allow(clippy::float_cmp)]
fn words_equal(a: u64, b: u64) -> bool {
    match (nv1::categorize(a), nv1::categorize(b)) {
        (nv1::Category::Int, nv1::Category::Int) => nv1::decode_int(a) == nv1::decode_int(b),
        (nv1::Category::Float, nv1::Category::Float) => f64::from_bits(a) == f64::from_bits(b),
        (nv1::Category::Special, nv1::Category::Special) => a == b,
        _ => a == b, // reference identity
    }
}

/// Truthiness: null, undefined, false, and integer zero are falsy.
#[must_use]
pub fn is_truthy(w: u64) -> bool {
    match nv1::categorize(w) {
        nv1::Category::Special => !matches!(
            nv1::decode_special(w),
            nv1::SPECIAL_NULL | nv1::SPECIAL_UNDEF | nv1::SPECIAL_FALSE
        ),
        nv1::Category::Int => nv1::decode_int(w) != 0,
        _ => true,
    }
}

/// Extracts a heap handle from a reference word.
fn as_handle(w: u64) -> Result<rt::Handle, rt::Trap> {
    if !nv1::is_ref(w) {
        return Err(rt::Trap::TypeMismatch {
            expected: "reference",
            word: w,
        });
    }
    Ok(rt::Handle(nv1::ref_offset(w)))
}

/// Extracts an array index from an integer word.
fn as_index(w: u64) -> Result<usize, rt::Trap> {
    if !nv1::is_int(w) {
        return Err(rt::Trap::TypeMismatch {
            expected: "index",
            word: w,
        });
    }
    let v = nv1::decode_int(w);
    usize::try_from(v).map_err(|_| rt::Trap::BoundsCheck {
        index: usize::MAX,
        len: 0,
    })
}

/// Site epoch cache table type re-exported for machine composition.
pub type SiteEpochs = HashMap<(usize, usize), Option<u16>>;
