//! T0: the reference interpreter over UGB programs.
// CEP:FILE crates/vortex-t0/src/lib.rs
// CEP:WHAT T0 interpreter: table-dispatch execution of UGB methods with the
//        BARRIER-1-corrected load barrier, epoch-based capability checks,
//        sampled back-edge counting for T4, AOSS numeric transitions,
//        exception handlers, and cross-method CALL through the program's
//        method table with a bounded frame stack.
// CEP:WHY T0 is the semantic reference for every higher tier: if T0's
//        behavior and an optimization disagree, T0 wins by definition.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE All failure paths surface as typed Trap values; handler stacks
//        are unwound on throw; recursion is bounded by the machine's
//        documented depth bound; no silent state corruption.
// CEP:ASSUMES Methods pass vortex-ugb verification before execution.
// CEP:COST Dispatch is a jump-table match; hot ops are branch-light. The
//        barrier adds at most one classify + one color test per ref load.
//        CALL/RETURN push/pop a frame: O(registers) per call.
// CEP:EVIDENCE cargo test -p vortex-t0
use std::collections::HashMap;

use vortex_nv1 as nv1;
use vortex_runtime as rt;
use vortex_ugb::{instr, Op};

/// The shared T0 semantic core (arithmetic, AOSS store policy, equality,
/// truthiness) reused verbatim by the higher-tier executors.
pub mod sem;

/// One activation frame.
#[derive(Debug)]
struct Frame {
    /// Method currently executing (index into the program).
    method: usize,
    /// Virtual registers (NV-1 words).
    regs: Vec<u64>,
    /// Open exception handlers (bytecode offsets of the Catch site).
    handlers: Vec<usize>,
    /// Resume point in the caller after this frame returns.
    resume: Option<usize>,
    /// Caller register that receives the returned word.
    caller_dst: usize,
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
    ///
    /// CALL convention (documented reference subset): `CALL dst, callee,
    /// argc` with `imm32u[0] = argc`; the caller stages arguments in its
    /// registers `0..argc` (the fixed argument-window convention), the
    /// callee receives them as its first `argc` registers, and `dst` of the
    /// caller receives the returned word. Recursion depth is bounded by
    /// `Machine::call_depth_bound` (bounded failure, Law 6).
    // CEP:WHAT Executes method_index with the given argument words to completion.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn run(&self, m: &mut rt::Machine, method_index: usize, args: &[u64]) -> ExecResult {
        let frame = self.make_frame(method_index, args)?;
        let mut frames = vec![frame];
        let mut pc = 0_usize;
        self.dispatch(m, &mut frames, &mut pc)
    }

    /// Resumes interpretation from an arbitrary bytecode pc with a supplied
    /// register frame: the deoptimization and OSR entry point (design.md
    /// OSR-1 section 7, section 4.4 "resuming exactly at the bytecode
    /// instruction that failed the assumption"). The frame words are the
    /// virtual registers; handlers start empty (a deopt into a try region
    /// re-executes the TryBegin).
    // CEP:WHAT Resumes interpretation from an arbitrary bytecode pc with a supplied register frame.
    // CEP:WHY The deopt/OSR contract of every compiled tier lands back in T0 at the failing pc; this method is that contract.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES pc is an instruction boundary and regs.len() equals the method's register_count.
    // CEP:COST Identical to run() from the resume point.
    // CEP:EVIDENCE cargo test -p vortex-t0; differential deopt tests in vortex-opt/vortex-t4
    pub fn resume(
        &self,
        m: &mut rt::Machine,
        method_index: usize,
        pc: usize,
        regs: Vec<u64>,
    ) -> ExecResult {
        let method = self
            .program
            .get(method_index)
            .ok_or(rt::Trap::InvalidEncoding { offset: 0 })?;
        if regs.len() != usize::from(method.register_count) {
            return Err(rt::Trap::InvalidEncoding { offset: pc });
        }
        let frame = Frame {
            method: method_index,
            regs,
            handlers: Vec::new(),
            resume: None,
            caller_dst: 0,
        };
        let mut frames = vec![frame];
        let mut cur_pc = pc;
        self.dispatch(m, &mut frames, &mut cur_pc)
    }

    /// The interpreter dispatch loop shared by `run` and `resume`.
    // CEP:WHAT The interpreter dispatch loop shared by run and resume.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn dispatch(&self, m: &mut rt::Machine, frames: &mut Vec<Frame>, pc: &mut usize) -> ExecResult {
        loop {
            let depth = frames.len();
            let top = frames.last_mut().expect("frame stack never empty");
            let method = self
                .program
                .get(top.method)
                .ok_or(rt::Trap::InvalidEncoding { offset: 0 })?;
            let (i, next) = instr::decode_at(&method.code, *pc)
                .map_err(|e| rt::Trap::InvalidEncoding { offset: e.offset })?;
            let pc_usize = *pc;
            match i.op {
                Op::LoadNull => top.regs[usize::from(i.regs[0])] = nv1::null_word(),
                Op::LoadTrue => top.regs[usize::from(i.regs[0])] = nv1::bool_word(true),
                Op::LoadFalse => top.regs[usize::from(i.regs[0])] = nv1::bool_word(false),
                Op::LoadInt32 => {
                    top.regs[usize::from(i.regs[0])] =
                        nv1::encode_int(i64::from(i.imm32)).unwrap_or_else(nv1::null_word)
                }
                Op::LoadFloat => {
                    top.regs[usize::from(i.regs[0])] = nv1::encode_f64(f64::from_bits(i.imm64))
                }
                Op::LoadConst => {
                    let c = method
                        .consts
                        .get(usize::from(i.pool))
                        .copied()
                        .ok_or(rt::Trap::InvalidEncoding { offset: pc_usize })?;
                    top.regs[usize::from(i.regs[0])] = match c {
                        vortex_ugb::Const::Int(v) => {
                            nv1::encode_int(v).unwrap_or_else(|| sem::box_int(m, v))
                        }
                        vortex_ugb::Const::Float(v) => nv1::encode_f64(v),
                    };
                }
                Op::Move => {
                    let v = top.regs[usize::from(i.regs[1])];
                    top.regs[usize::from(i.regs[0])] = v;
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div => {
                    let dst = usize::from(i.regs[0]);
                    let a = top.regs[usize::from(i.regs[1])];
                    let b = top.regs[usize::from(i.regs[2])];
                    top.regs[dst] = sem::arith(m, i.op, a, b)?;
                }
                Op::Neg => {
                    let a = top.regs[usize::from(i.regs[1])];
                    let zero = nv1::encode_int(0).expect("zero encodes");
                    top.regs[usize::from(i.regs[0])] = sem::arith(m, Op::Sub, zero, a)?;
                }
                Op::Eq | Op::Lt | Op::Le => {
                    let dst = usize::from(i.regs[0]);
                    let a = top.regs[usize::from(i.regs[1])];
                    let b = top.regs[usize::from(i.regs[2])];
                    let r = if i.op == Op::Eq {
                        sem::words_equal(a, b)
                    } else {
                        let x = sem::numeric_value(m, a)?;
                        let y = sem::numeric_value(m, b)?;
                        match x.partial_cmp(&y) {
                            Some(core::cmp::Ordering::Less) => i.op == Op::Lt || i.op == Op::Le,
                            Some(core::cmp::Ordering::Equal) => i.op == Op::Le,
                            _ => false,
                        }
                    };
                    top.regs[dst] = nv1::bool_word(r);
                }
                Op::IsNull => {
                    let a = top.regs[usize::from(i.regs[1])];
                    top.regs[usize::from(i.regs[0])] = nv1::bool_word(a == nv1::null_word());
                }
                Op::Jump | Op::JumpTrue | Op::JumpFalse => {
                    let target = method.branch_target(&i, pc_usize, next - pc_usize);
                    let taken = match i.op {
                        Op::Jump => true,
                        Op::JumpTrue => sem::is_truthy(top.regs[usize::from(i.regs[0])]),
                        _ => !sem::is_truthy(top.regs[usize::from(i.regs[0])]),
                    };
                    if taken {
                        if target < pc_usize {
                            // Back-edge: sampled counter for T4 (PROF-1 3.2).
                            // The per-site slot index is the ordinal of this
                            // backward-jump instruction in the stream.
                            let site = backward_site_ordinal(method, pc_usize);
                            let _request = m.profile.backedge(site);
                        }
                        *pc = target;
                        continue;
                    }
                }
                Op::Call => {
                    let callee = usize::from(i.regs[1]);
                    if callee >= self.program.len() {
                        return Err(rt::Trap::CalleeOutOfRange {
                            callee,
                            methods: self.program.len(),
                        });
                    }
                    if depth >= m.call_depth_bound {
                        return Err(rt::Trap::CallStackOverflow {
                            bound: m.call_depth_bound,
                        });
                    }
                    let argc = i.imm32u[0] as usize;
                    let callee_method = &self.program[callee];
                    if usize::from(callee_method.arg_count) < argc {
                        return Err(rt::Trap::InvalidEncoding { offset: pc_usize });
                    }
                    let site = (top.method, pc_usize);
                    let counter = m.call_counts.entry(site).or_insert(0);
                    *counter = counter.saturating_add(1);
                    // Arguments: caller registers 0..argc (fixed window).
                    let args: Vec<u64> = top.regs[..argc].to_vec();
                    let resume = next;
                    let dst = usize::from(i.regs[0]);
                    let mut callee_frame = self.make_frame(callee, &args)?;
                    callee_frame.resume = Some(resume);
                    callee_frame.caller_dst = dst;
                    frames.push(callee_frame);
                    *pc = 0;
                    continue;
                }
                Op::Return => {
                    let returned = top.regs[usize::from(i.regs[0])];
                    let finished = frames.pop().expect("frame stack never empty");
                    match finished.resume {
                        Some(resume) => {
                            // Resume the caller; write the return value into
                            // the caller's dst register recorded at the call.
                            let caller = frames.last_mut().expect("caller present");
                            caller.regs[finished.caller_dst] = returned;
                            *pc = resume;
                            continue;
                        }
                        None => return Ok(returned),
                    }
                }
                Op::AllocArray => {
                    let len = i.imm32u[0] as usize;
                    let nursery = m.heap.nursery_page().ok_or(rt::Trap::OutOfMemory)?;
                    let h = m
                        .heap
                        .alloc_array(rt::layout::GENERIC_ARRAY, len, nursery)?;
                    top.regs[usize::from(i.regs[0])] =
                        nv1::encode_ref(h.0, 0).expect("valid handle");
                }
                Op::LoadIndex => {
                    let arr = sem::as_handle(top.regs[usize::from(i.regs[1])])?;
                    let idx = sem::as_index(top.regs[usize::from(i.regs[2])])?;
                    let v = m.heap.load_array_element(arr, idx, &mut m.gc.stats)?;
                    top.regs[usize::from(i.regs[0])] = v;
                }
                Op::StoreIndex => {
                    let arr = sem::as_handle(top.regs[usize::from(i.regs[0])])?;
                    let idx = sem::as_index(top.regs[usize::from(i.regs[1])])?;
                    let val = top.regs[usize::from(i.regs[2])];
                    self.store_element(m, arr, idx, val)?;
                }
                Op::LoadLength => {
                    let arr = sem::as_handle(top.regs[usize::from(i.regs[1])])?;
                    let len = m.heap.array_len(arr);
                    top.regs[usize::from(i.regs[0])] =
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
                    top.regs[usize::from(i.regs[0])] =
                        nv1::encode_ref(h.0, 0).expect("valid handle");
                }
                Op::CapCheck => {
                    let cap_word = top.regs[usize::from(i.regs[0])];
                    let h = sem::as_handle(cap_word)?;
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
                    m.site_epochs.insert((top.method, site), cache.cached_epoch);
                }
                Op::CapRevoke => {
                    let cap_word = top.regs[usize::from(i.regs[0])];
                    let h = sem::as_handle(cap_word)?;
                    let id = rt::CapId(m.heap.read_payload(h, 1) as u32);
                    m.caps.revoke(id);
                }
                Op::Throw => {
                    let exc = top.regs[usize::from(i.regs[0])];
                    match top.handlers.pop() {
                        Some(catch_pc) => {
                            m.pending_exception = Some(exc);
                            *pc = catch_pc;
                            continue;
                        }
                        None => return Err(rt::Trap::UnhandledException),
                    }
                }
                Op::TryBegin => {
                    let target = method.branch_target(&i, pc_usize, next - pc_usize);
                    top.handlers.push(target);
                }
                Op::TryEnd => {
                    top.handlers.pop();
                }
                Op::Catch => {
                    let exc = m.pending_exception.take().unwrap_or_else(nv1::null_word);
                    top.regs[usize::from(i.regs[0])] = exc;
                }
                Op::Safepoint | Op::Nop => { /* documented no-op */ }
                Op::Halt => return Ok(nv1::null_word()),
            }
            *pc = next;
        }
    }

    /// Builds one activation frame: undefined-initialized registers, caller
    /// arguments in `0..arg_count`, and the caller resume bookkeeping.
    // CEP:WHAT Builds one activation frame: undefined-initialized registers, caller arguments in 0..arg_count, and the caller resume bookkeeping.
    // CEP:WHY Frame construction is shared by the entry call and nested CALLs.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST O(register_count) per frame.
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn make_frame(&self, method_index: usize, args: &[u64]) -> Result<Frame, rt::Trap> {
        let method = self
            .program
            .get(method_index)
            .ok_or(rt::Trap::InvalidEncoding { offset: 0 })?;
        let mut regs = vec![nv1::undefined_word(); usize::from(method.register_count)];
        for (i, &a) in args.iter().enumerate().take(usize::from(method.arg_count)) {
            regs[i] = a;
        }
        Ok(Frame {
            method: method_index,
            regs,
            handlers: Vec::new(),
            resume: None,
            caller_dst: 0,
        })
    }

    /// Store path with the full AOSS policy: numeric elision, reverse
    /// transition on domain violation, transition on homogeneity window.
    ///
    /// Delegates to [`sem::store_element`], the shared reference semantics
    /// also composed by the T1/T2/T4 executors.
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
        sem::store_element(m, arr, idx, val)
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

/// Site epoch cache table type re-exported for machine composition.
pub type SiteEpochs = HashMap<(usize, usize), Option<u16>>;
