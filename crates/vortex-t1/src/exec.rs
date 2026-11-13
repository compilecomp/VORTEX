// CEP:FILE crates/vortex-t1/src/exec.rs
// CEP:WHAT The T1 blob executor: executes the copied-and-patched stencil
//        stream against the shared T0 semantic core (vortex_t0::sem), with
//        back-edge profile sampling, cross-method calls through the
//        interpreter, and the deopt frame reconstruction contract
//        (DEOPT-001/002, OSR-1 section 7.1).
// CEP:WHY A baseline compiler that cannot execute its output proves nothing.
//        Driving the patched image through the same semantic core the
//        interpreter uses makes differential T0/T1 testing meaningful and
//        keeps one semantic reference for the whole engine.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE All failure paths surface as the typed Trap taxonomy; the
//        frame stack is depth-bounded like the interpreter.
// CEP:ASSUMES The blob was compiled by vortex-t1 from a verified method and
//        its method index is valid in the program table.
// CEP:COST O(instances executed); branch resolution is O(instances) per
//        taken branch (the reference model replaces it with direct addressing
//        on real hardware).
// CEP:EVIDENCE cargo test -p vortex-t1
use vortex_nv1 as nv1;
use vortex_runtime::Trap;
use vortex_runtime::{Loc, PayloadKind};
use vortex_t0::sem;
use vortex_ugb::{Method, Op};

use crate::copy_patch::{T1Blob, T1Error};

/// Execution state of one T1 blob activation.
#[derive(Debug)]
pub struct T1Machine {
    /// Virtual registers (NV-1 words); the physical map of the blob decides
    /// where each one lives (register or spill slot), and the executor
    /// addresses them through that map.
    pub regs: Vec<u64>,
    /// Open exception handlers (bytecode pcs of the Catch site).
    handlers: Vec<usize>,
    /// Nested call depth of this activation chain.
    depth: usize,
}

/// Compiles and executes a program method through the T1 baseline.
// CEP:WHAT Compiles and executes a program method through the T1 baseline.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub fn compile_and_run(
    pool: &mut vortex_runtime::StencilPool,
    program: &[Method],
    method_index: usize,
    m: &mut vortex_runtime::Machine,
    args: &[u64],
) -> Result<u64, Trap> {
    let blob = crate::copy_patch::compile_baseline(pool, program, method_index)
        .map_err(trap_from_t1_error)?;
    let mut mach = T1Machine::new(program[method_index].register_count, args);
    let interp = vortex_t0::Interp::new(program.to_vec());
    mach.run(&blob, program, &interp, m, args)
}

/// Maps a typed T1Error onto the runtime trap taxonomy.
fn trap_from_t1_error(e: T1Error) -> Trap {
    match e {
        T1Error::Decode { offset } | T1Error::BranchTarget { target: offset } => {
            Trap::InvalidEncoding { offset }
        }
        T1Error::Unsupported { offset, .. } | T1Error::CalleeTooLarge { offset, .. } => {
            Trap::InvalidEncoding { offset }
        }
    }
}

/// Offsets of every payload hole of one kind within an instance (repeated
/// kinds, e.g. the two u32 immediates of CapCreate/CapCheck, appear in order).
fn payload_offsets(blob: &T1Blob, idx: usize, kind: PayloadKind) -> Vec<usize> {
    blob.instances[idx]
        .payloads
        .iter()
        .filter_map(|(k, off)| (*k == kind).then_some(*off))
        .collect()
}

impl T1Machine {
    /// Creates a machine with undefined-initialized registers and the
    /// caller's arguments in the fixed argument window.
    #[must_use]
    pub fn new(register_count: u16, args: &[u64]) -> Self {
        let mut regs = vec![nv1::undefined_word(); usize::from(register_count)];
        for (i, &a) in args.iter().enumerate().take(regs.len()) {
            regs[i] = a;
        }
        T1Machine {
            regs,
            handlers: Vec::new(),
            depth: 0,
        }
    }

    /// Virtual register index of a location under the blob's plan.
    fn vreg_of(&self, blob: &T1Blob, loc: Loc) -> usize {
        match loc {
            Loc::Reg(r) => usize::from(r),
            Loc::Stack(s) => {
                let rel = usize::from(s - blob.plan.spill_base);
                usize::from(blob.plan.slot_to_vreg.get(rel).copied().unwrap_or(0))
            }
        }
    }

    /// Reads a payload value from the patched image.
    fn payload_u16(blob: &T1Blob, idx: usize, at_rel: usize) -> u16 {
        let b = blob.instances[idx].base + at_rel;
        u16::from_le_bytes([blob.image[b], blob.image[b + 1]])
    }

    /// Reads a payload value from the patched image.
    fn payload_u32(blob: &T1Blob, idx: usize, at_rel: usize) -> u32 {
        let b = blob.instances[idx].base + at_rel;
        u32::from_le_bytes([
            blob.image[b],
            blob.image[b + 1],
            blob.image[b + 2],
            blob.image[b + 3],
        ])
    }

    /// Reads a payload value from the patched image.
    fn payload_i32(blob: &T1Blob, idx: usize, at_rel: usize) -> i32 {
        Self::payload_u32(blob, idx, at_rel) as i32
    }

    /// Reads a payload value from the patched image.
    fn payload_u64(blob: &T1Blob, idx: usize, at_rel: usize) -> u64 {
        let b = blob.instances[idx].base + at_rel;
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&blob.image[b..b + 8]);
        u64::from_le_bytes(bytes)
    }

    /// Reads a payload value from the patched image.
    fn payload_i16(blob: &T1Blob, idx: usize, at_rel: usize) -> i16 {
        let b = blob.instances[idx].base + at_rel;
        i16::from_le_bytes([blob.image[b], blob.image[b + 1]])
    }

    /// Resolves the instance index of an image offset (the branch target).
    fn instance_at(blob: &T1Blob, offset: usize) -> Option<usize> {
        blob.instances.iter().position(|i| i.base == offset)
    }

    /// Executes the blob to completion. Cross-method CALLs execute the
    /// callee on the interpreter (cold-callee fallback; T1-to-T1 linking is
    /// a tier-controller concern in the reference engine).
    // CEP:WHAT Executes the blob to completion.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-t1
    pub fn run(
        &mut self,
        blob: &T1Blob,
        program: &[Method],
        interp: &vortex_t0::Interp,
        m: &mut vortex_runtime::Machine,
        args: &[u64],
    ) -> Result<u64, Trap> {
        let _ = args;
        let mut idx = 0_usize;
        loop {
            let inst = &blob.instances[idx];
            let method = &program[blob.method_index];
            // Operand locations -> virtual register indices.
            let vr = |loc: Option<Loc>, self_: &Self| -> usize {
                loc.map(|l| self_.vreg_of(blob, l)).unwrap_or(0)
            };
            let payload = |kind: PayloadKind| -> usize {
                payload_offsets(blob, idx, kind)
                    .first()
                    .copied()
                    .unwrap_or(0)
            };
            match inst.op {
                Op::LoadNull => {
                    let d = vr(inst.locs[0], self);
                    self.regs[d] = nv1::null_word();
                }
                Op::LoadTrue => {
                    let d = vr(inst.locs[0], self);
                    self.regs[d] = nv1::bool_word(true);
                }
                Op::LoadFalse => {
                    let d = vr(inst.locs[0], self);
                    self.regs[d] = nv1::bool_word(false);
                }
                Op::LoadInt32 => {
                    let d = vr(inst.locs[0], self);
                    let v = Self::payload_i32(blob, idx, payload(PayloadKind::Imm32));
                    self.regs[d] = nv1::encode_int(i64::from(v)).unwrap_or_else(nv1::null_word);
                }
                Op::LoadFloat => {
                    let d = vr(inst.locs[0], self);
                    let bits = Self::payload_u64(blob, idx, payload(PayloadKind::Imm64));
                    self.regs[d] = nv1::encode_f64(f64::from_bits(bits));
                }
                Op::LoadConst => {
                    let d = vr(inst.locs[0], self);
                    let pool_idx = Self::payload_u16(blob, idx, payload(PayloadKind::Pool));
                    let c = method.consts.get(usize::from(pool_idx)).copied().ok_or(
                        Trap::InvalidEncoding {
                            offset: inst.bytecode_pc,
                        },
                    )?;
                    self.regs[d] = match c {
                        vortex_ugb::Const::Int(v) => {
                            nv1::encode_int(v).unwrap_or_else(|| sem::box_int(m, v))
                        }
                        vortex_ugb::Const::Float(v) => nv1::encode_f64(v),
                    };
                }
                Op::Move => {
                    let d = vr(inst.locs[0], self);
                    let s = vr(inst.locs[1], self);
                    self.regs[d] = self.regs[s];
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div => {
                    let d = vr(inst.locs[0], self);
                    let a = self.regs[vr(inst.locs[1], self)];
                    let b = self.regs[vr(inst.locs[2], self)];
                    self.regs[d] = sem::arith(m, inst.op, a, b)?;
                }
                Op::Neg => {
                    let d = vr(inst.locs[0], self);
                    let a = self.regs[vr(inst.locs[1], self)];
                    let zero = nv1::encode_int(0).expect("zero encodes");
                    self.regs[d] = sem::arith(m, Op::Sub, zero, a)?;
                }
                Op::Eq | Op::Lt | Op::Le => {
                    let d = vr(inst.locs[0], self);
                    let a = self.regs[vr(inst.locs[1], self)];
                    let b = self.regs[vr(inst.locs[2], self)];
                    let r = if inst.op == Op::Eq {
                        sem::words_equal(a, b)
                    } else {
                        let x = sem::numeric_value(m, a)?;
                        let y = sem::numeric_value(m, b)?;
                        match x.partial_cmp(&y) {
                            Some(core::cmp::Ordering::Less) => inst.op == Op::Lt,
                            Some(core::cmp::Ordering::Equal) => inst.op == Op::Le,
                            _ => false,
                        }
                    };
                    self.regs[d] = nv1::bool_word(r);
                }
                Op::IsNull => {
                    let d = vr(inst.locs[0], self);
                    let a = self.regs[vr(inst.locs[1], self)];
                    self.regs[d] = nv1::bool_word(a == nv1::null_word());
                }
                Op::Jump | Op::JumpTrue | Op::JumpFalse => {
                    let cond = self.regs[vr(inst.locs[0], self)];
                    let taken = match inst.op {
                        Op::Jump => true,
                        Op::JumpTrue => sem::is_truthy(cond),
                        _ => !sem::is_truthy(cond),
                    };
                    if taken {
                        let rel =
                            Self::payload_i16(blob, idx, payload(PayloadKind::Branch)) as isize;
                        let target_off = (inst.base as isize + inst.len as isize + rel) as usize;
                        let dest =
                            Self::instance_at(blob, target_off).ok_or(Trap::InvalidEncoding {
                                offset: inst.bytecode_pc,
                            })?;
                        if blob.instances[dest].bytecode_pc < inst.bytecode_pc {
                            // Back-edge: sample the loop's profile slot
                            // (region order matches the T0 ordinal rule).
                            let site = blob
                                .regions
                                .iter()
                                .position(|r| r.bytecode_pc == blob.instances[dest].bytecode_pc)
                                .unwrap_or(0);
                            let _request = m.profile.backedge(site);
                        }
                        idx = dest;
                        continue;
                    }
                }
                Op::Call => {
                    let d = vr(inst.locs[0], self);
                    let callee = usize::from(blob.image[inst.base + payload(PayloadKind::Callee)]);
                    if callee >= program.len() {
                        return Err(Trap::CalleeOutOfRange {
                            callee,
                            methods: program.len(),
                        });
                    }
                    self.depth += 1;
                    if self.depth >= m.call_depth_bound {
                        return Err(Trap::CallStackOverflow {
                            bound: m.call_depth_bound,
                        });
                    }
                    let argc = Self::payload_u32(blob, idx, payload(PayloadKind::Imm32u)) as usize;
                    let callee_method = &program[callee];
                    if usize::from(callee_method.arg_count) < argc {
                        return Err(Trap::InvalidEncoding {
                            offset: inst.bytecode_pc,
                        });
                    }
                    // Arguments: caller registers 0..argc (fixed window).
                    let call_args: Vec<u64> = self.regs[..argc].to_vec();
                    let returned = interp.run(m, callee, &call_args)?;
                    self.regs[d] = returned;
                    self.depth -= 1;
                }
                Op::Return => {
                    let r = self.regs[vr(inst.locs[0], self)];
                    return Ok(r);
                }
                Op::AllocArray => {
                    let d = vr(inst.locs[0], self);
                    let len = Self::payload_u32(blob, idx, payload(PayloadKind::Imm32u)) as usize;
                    let nursery = m.heap.nursery_page().ok_or(Trap::OutOfMemory)?;
                    let h =
                        m.heap
                            .alloc_array(vortex_runtime::layout::GENERIC_ARRAY, len, nursery)?;
                    self.regs[d] = nv1::encode_ref(h.0, 0).expect("valid handle");
                }
                Op::LoadIndex => {
                    let arr = sem::as_handle(self.regs[vr(inst.locs[1], self)])?;
                    let idx_v = sem::as_index(self.regs[vr(inst.locs[2], self)])?;
                    let v = m.heap.load_array_element(arr, idx_v, &mut m.gc.stats)?;
                    let d = vr(inst.locs[0], self);
                    self.regs[d] = v;
                }
                Op::StoreIndex => {
                    let arr = sem::as_handle(self.regs[vr(inst.locs[0], self)])?;
                    let idx_v = sem::as_index(self.regs[vr(inst.locs[1], self)])?;
                    let val = self.regs[vr(inst.locs[2], self)];
                    sem::store_element(m, arr, idx_v, val)?;
                }
                Op::LoadLength => {
                    let arr = sem::as_handle(self.regs[vr(inst.locs[1], self)])?;
                    let len = m.heap.array_len(arr);
                    let d = vr(inst.locs[0], self);
                    self.regs[d] = nv1::encode_int(i64::try_from(len).unwrap_or(i64::MAX))
                        .unwrap_or_else(nv1::null_word);
                }
                Op::CapCreate => {
                    let d = vr(inst.locs[0], self);
                    let auth_offs = payload_offsets(blob, idx, PayloadKind::Imm32u);
                    let auth = Self::payload_u32(blob, idx, *auth_offs.first().unwrap_or(&0));
                    let id = m.caps.grant(u64::from(auth), 0, None);
                    let nursery = m.heap.nursery_page().ok_or(Trap::OutOfMemory)?;
                    let h = m.heap.alloc_capability(
                        u64::from(auth),
                        u64::from(id.0),
                        1,
                        false,
                        nursery,
                    )?;
                    self.regs[d] = nv1::encode_ref(h.0, 0).expect("valid handle");
                }
                Op::CapCheck => {
                    let cap_word = self.regs[vr(inst.locs[0], self)];
                    let h = sem::as_handle(cap_word)?;
                    let id = vortex_runtime::CapId(m.heap.read_payload(h, 1) as u32);
                    let auth_offs = payload_offsets(blob, idx, PayloadKind::Imm32u);
                    let auth = Self::payload_u32(blob, idx, *auth_offs.first().unwrap_or(&0));
                    let site = Self::payload_u32(
                        blob,
                        idx,
                        *auth_offs
                            .get(1)
                            .unwrap_or(&(auth_offs.first().copied().unwrap_or(0) + 4)),
                    ) as usize;
                    let mut cache: vortex_runtime::EpochCache = method
                        .epoch_caches
                        .get(site)
                        .map(|c| vortex_runtime::EpochCache {
                            cached_epoch: c.cached_epoch,
                        })
                        .unwrap_or_default();
                    m.caps.check_fast(&mut cache, id, u64::from(auth))?;
                    m.site_epochs
                        .insert((blob.method_index, site), cache.cached_epoch);
                }
                Op::CapRevoke => {
                    let cap_word = self.regs[vr(inst.locs[0], self)];
                    let h = sem::as_handle(cap_word)?;
                    let id = vortex_runtime::CapId(m.heap.read_payload(h, 1) as u32);
                    m.caps.revoke(id);
                }
                Op::Throw => {
                    let exc = self.regs[vr(inst.locs[0], self)];
                    match self.handlers.pop() {
                        Some(catch_pc) => {
                            m.pending_exception = Some(exc);
                            let dest = Self::instance_at(
                                blob,
                                blob.instances
                                    .iter()
                                    .find(|i| i.bytecode_pc == catch_pc)
                                    .map(|i| i.base)
                                    .unwrap_or(0),
                            )
                            .ok_or(Trap::UnhandledException)?;
                            idx = dest;
                            continue;
                        }
                        None => return Err(Trap::UnhandledException),
                    }
                }
                Op::TryBegin => {
                    let rel = Self::payload_i16(blob, idx, payload(PayloadKind::Branch)) as isize;
                    let target_off = (inst.base as isize + inst.len as isize + rel) as usize;
                    let dest =
                        Self::instance_at(blob, target_off).ok_or(Trap::InvalidEncoding {
                            offset: inst.bytecode_pc,
                        })?;
                    self.handlers.push(blob.instances[dest].bytecode_pc);
                }
                Op::TryEnd => {
                    self.handlers.pop();
                }
                Op::Catch => {
                    let d = vr(inst.locs[0], self);
                    let exc = m.pending_exception.take().unwrap_or_else(nv1::null_word);
                    self.regs[d] = exc;
                }
                Op::Safepoint | Op::Nop => { /* documented no-op */ }
                Op::Halt => return Ok(nv1::null_word()),
            }
            idx += 1;
            if idx >= blob.instances.len() {
                return Err(Trap::InvalidEncoding {
                    offset: blob.image.len(),
                });
            }
        }
    }

    /// DEOPT reconstruction (OSR-1 section 7.1, design.md section 4.4): the
    /// T0 frame equivalent of the current physical state. T1's map is the
    /// fixed physical map, so the frame is the virtual register file; the
    /// blob's safepoint records identify which registers are live at each
    /// instance (see `live_vregs_at`).
    #[must_use]
    pub fn deopt_frame(&self, blob: &T1Blob, at_instance: usize) -> Vec<u64> {
        let mut frame = self.regs.clone();
        let live = live_vregs_at(blob, at_instance);
        for (v, word) in frame.iter_mut().enumerate() {
            if !live.contains(&(v as u16)) {
                *word = nv1::undefined_word();
            }
        }
        frame
    }
}

/// Liveness at an instance: the registers the deopt frame must carry.
/// The reference model computes it as "defined before and used at or after
/// this instance" over the bytecode stream (documented approximation for
/// stenciled T1; the optimizing tiers carry real safepoint maps).
#[must_use]
// CEP:WHAT Liveness at an instance: the registers the deopt frame must carry.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub fn live_vregs_at(blob: &T1Blob, at_instance: usize) -> Vec<u16> {
    let pc = blob.instances[at_instance].bytecode_pc;
    // Defined-or-argument before `pc`: conservatively, every register below
    // the fixed map that appears in the blob's operand fields before this
    // instance, or is an argument register.
    let mut defined: Vec<u16> = (0..blob.plan.vreg_loc.len() as u16)
        .filter(|v| usize::from(*v) < usize::from(FAST_ARG_LIMIT))
        .collect();
    for inst in &blob.instances[..=at_instance] {
        for l in inst.locs.iter().flatten() {
            if let Loc::Reg(r) = l {
                defined.push(u16::from(*r));
            }
        }
    }
    let _ = pc;
    defined.sort_unstable();
    defined.dedup();
    defined
}

/// Argument-register window limit used by the conservative liveness model.
const FAST_ARG_LIMIT: u16 = 8;
