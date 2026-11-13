// CEP:FILE crates/vortex-t1/src/fallback.rs
// CEP:WHAT The T1c fallback baseline: lightweight CFG, per-instruction
//        liveness, live intervals, Poletto-style linear scan over the fixed
//        fast-map register file, and stencil emission with the computed
//        allocation (design.md section 5 revised T1 structure).
// CEP:WHY The fixed map cannot be the whole story: high-register-pressure
//        methods degrade it. The fallback keeps the latency win bounded
//        instead of pathological: one liveness pass plus a linear scan
//        replace the fixed map, and the same stencil machinery emits code.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Undecodable bytecode surfaces as T1Error; allocation always
//        succeeds (registers beyond the fast map spill to slots).
// CEP:ASSUMES The method passed vortex-ugb verification.
// CEP:COST O(instructions * registers) for liveness, O(n log n) for the
//        interval sort; allocation is O(intervals * fast-map width).
// CEP:EVIDENCE cargo test -p vortex-t1 (differential T1c tests)
use vortex_runtime::{Loc, StencilPool};
use vortex_ugb::{Method, Op};

use crate::copy_patch::{compile_with_plan, decode_all, Decoded, LocationPlan, T1Error};
use crate::plan::{T1Path, FAST_MAP_REGS};

/// Compiles a method through the T1c fallback path: liveness, linear scan,
/// then the shared copy-and-patch emitter with the computed allocation.
// CEP:WHAT Compiles a method through the T1c fallback path: liveness, linear scan, then the shared copy-and-patch emitter with the computed allocation.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub fn compile_t1c(
    pool: &mut StencilPool,
    program: &[Method],
    method_index: usize,
) -> Result<crate::copy_patch::T1Blob, T1Error> {
    let method = program
        .get(method_index)
        .ok_or(T1Error::Decode { offset: 0 })?;
    let decoded = decode_all(method)?;
    let plan = allocate(method, &decoded);
    compile_with_plan(pool, method, method_index, T1Path::C, plan)
}

/// One live interval: [first .. last] instruction ordinals (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interval {
    /// Virtual register.
    vreg: u16,
    /// First ordinal where the register is live.
    first: usize,
    /// Last ordinal where the register is live.
    last: usize,
}

/// Computes the location plan: block liveness fixpoint, live intervals,
/// then Poletto-style linear scan with the fast-map width.
// CEP:WHAT Computes the location plan: block liveness fixpoint, live intervals, then Poletto-style linear scan with the fast-map width.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
fn allocate(method: &Method, decoded: &[Decoded]) -> LocationPlan {
    let n = decoded.len();
    let regs = usize::from(method.register_count);

    // use/def sets per instruction.
    let mut uses: Vec<Vec<u16>> = vec![Vec::new(); n];
    let mut defs: Vec<Vec<u16>> = vec![Vec::new(); n];
    for (i, d) in decoded.iter().enumerate() {
        uses[i] = instruction_uses(&d.instr);
        defs[i] = instruction_defs(&d.instr);
    }

    // Basic blocks: leaders at 0, branch targets, and after branches.
    let mut leaders = vec![false; n];
    leaders[0] = true;
    for (i, d) in decoded.iter().enumerate() {
        let branches = vortex_ugb::sig(d.instr.op).branches > 0;
        if branches {
            if let Some(t) = d.target {
                // Instruction ordinal of the target.
                if let Some(ti) = decoded.iter().position(|x| x.pc == t) {
                    leaders[ti] = true;
                }
            }
            if i + 1 < n {
                leaders[i + 1] = true;
            }
        }
    }
    let mut blocks: Vec<(usize, usize)> = Vec::new(); // [start, end_excl)
    let mut start = 0;
    for (i, l) in leaders.iter().enumerate() {
        if *l && i > start {
            blocks.push((start, i));
            start = i;
        }
    }
    blocks.push((start, n));

    // Successors per block.
    let block_of = |ordinal: usize| -> usize {
        blocks
            .iter()
            .position(|&(s, e)| ordinal >= s && ordinal < e)
            .unwrap_or(blocks.len() - 1)
    };
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); blocks.len()];
    for (bi, &(_s, e)) in blocks.iter().enumerate() {
        let last = e - 1;
        let d = &decoded[last];
        if d.instr.op == Op::Return || d.instr.op == Op::Halt {
            continue;
        }
        if let Some(t) = d.target {
            if let Some(ti) = decoded.iter().position(|x| x.pc == t) {
                succs[bi].push(block_of(ti));
            }
        }
        // Conditional branches and non-branch fall-through.
        if !(d.instr.op == Op::Jump) && last + 1 < n {
            succs[bi].push(block_of(last + 1));
        }
    }

    // Block-level liveness fixpoint (backward).
    let mut live_in: Vec<Vec<bool>> = vec![vec![false; regs]; blocks.len()];
    let mut live_out: Vec<Vec<bool>> = vec![vec![false; regs]; blocks.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for bi in (0..blocks.len()).rev() {
            let (s, e) = blocks[bi];
            let mut out = vec![false; regs];
            for &si in &succs[bi] {
                for v in 0..regs {
                    out[v] |= live_in[si][v];
                }
            }
            let mut inp = out.clone();
            // live_in = use(block) ∪ (live_out - def(block))
            for i in (s..e).rev() {
                for &d in &defs[i] {
                    inp[usize::from(d)] = false;
                }
                for &u in &uses[i] {
                    inp[usize::from(u)] = true;
                }
            }
            if out != live_out[bi] || inp != live_in[bi] {
                live_out[bi] = out;
                live_in[bi] = inp;
                changed = true;
            }
        }
    }

    // Per-instruction backward liveness walk -> intervals.
    let mut first_seen: Vec<Option<usize>> = vec![None; regs];
    let mut last_seen: Vec<Option<usize>> = vec![None; regs];
    for (bi, &(s, e)) in blocks.iter().enumerate().rev() {
        let mut live = live_out[bi].clone();
        for i in (s..e).rev() {
            // Registers live at this point (live-before the instruction).
            for &d in &defs[i] {
                live[usize::from(d)] = false;
            }
            for &u in &uses[i] {
                live[usize::from(u)] = true;
            }
            for v in 0..regs {
                if live[v] || uses[i].contains(&(v as u16)) || defs[i].contains(&(v as u16)) {
                    last_seen[v] = Some(last_seen[v].max(Some(i)).unwrap_or(i));
                    first_seen[v] = Some(first_seen[v].map_or(i, |f| f.min(i)));
                }
            }
        }
    }

    let mut intervals: Vec<Interval> = (0..regs)
        .filter_map(|v| match (first_seen[v], last_seen[v]) {
            (Some(f), Some(l)) => Some(Interval {
                vreg: v as u16,
                first: f,
                last: l,
            }),
            _ => None,
        })
        .collect();
    intervals.sort_by_key(|iv| (iv.first, iv.vreg));

    // Poletto-style linear scan over the fast-map width.
    let fast = usize::from(FAST_MAP_REGS);
    let mut free: Vec<u8> = (0..fast as u8).rev().collect();
    // active: (interval, reg)
    let mut active: Vec<(Interval, u8)> = Vec::new();
    let mut vreg_loc: Vec<Loc> = vec![Loc::Reg(0); regs];
    let mut slot_to_vreg: Vec<u16> = Vec::new();
    let mut next_slot = FAST_MAP_REGS;

    for iv in &intervals {
        // Expire intervals that ended before this one starts.
        active.retain(|(a, r)| {
            if a.last < iv.first {
                free.push(*r);
                false
            } else {
                true
            }
        });
        if let Some(r) = free.pop() {
            vreg_loc[usize::from(iv.vreg)] = Loc::Reg(r);
            active.push((*iv, r));
        } else {
            // Spill the interval with the farthest end.
            let farthest = active
                .iter()
                .enumerate()
                .max_by_key(|(_, (a, _))| a.last)
                .filter(|(_, (a, _))| a.last > iv.last)
                .map(|(i, _)| i);
            if let Some(fi) = farthest {
                let (evicted_iv, evicted_reg) = active.remove(fi);
                vreg_loc[usize::from(evicted_iv.vreg)] = Loc::Stack(next_slot);
                slot_to_vreg.push(evicted_iv.vreg);
                next_slot += 1;
                vreg_loc[usize::from(iv.vreg)] = Loc::Reg(evicted_reg);
                active.push((*iv, evicted_reg));
            } else {
                vreg_loc[usize::from(iv.vreg)] = Loc::Stack(next_slot);
                slot_to_vreg.push(iv.vreg);
                next_slot += 1;
            }
        }
    }

    LocationPlan {
        vreg_loc,
        slot_to_vreg,
        spill_base: FAST_MAP_REGS,
    }
}

/// Registers an instruction reads (excluding the CALL callee byte, which is
/// a method index, not a register).
// CEP:WHAT Registers an instruction reads (excluding the CALL callee byte, which is a method index, not a register).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
fn instruction_uses(i: &vortex_ugb::instr::Instr) -> Vec<u16> {
    let s = vortex_ugb::sig(i.op);
    let mut out = Vec::new();
    for (slot, r) in i.regs.iter().enumerate().take(usize::from(s.regs)) {
        let is_dst = dst_slots(i.op).contains(&(slot as u8));
        if is_dst {
            continue;
        }
        if i.op == Op::Call && slot == 1 {
            continue; // callee index, not a register use
        }
        out.push(*r);
    }
    out
}

/// Registers an instruction writes.
fn instruction_defs(i: &vortex_ugb::instr::Instr) -> Vec<u16> {
    let s = vortex_ugb::sig(i.op);
    (0..usize::from(s.regs))
        .filter(|slot| dst_slots(i.op).contains(&(*slot as u8)))
        .map(|slot| i.regs[slot])
        .collect()
}

/// Operand slots that are written by the opcode.
fn dst_slots(op: Op) -> Vec<u8> {
    match op {
        Op::LoadNull
        | Op::LoadTrue
        | Op::LoadFalse
        | Op::LoadInt32
        | Op::LoadFloat
        | Op::LoadConst
        | Op::Move
        | Op::Add
        | Op::Sub
        | Op::Mul
        | Op::Div
        | Op::Neg
        | Op::Eq
        | Op::Lt
        | Op::Le
        | Op::IsNull
        | Op::AllocArray
        | Op::LoadIndex
        | Op::LoadLength
        | Op::CapCreate
        | Op::Catch => vec![0],
        Op::StoreIndex | Op::Call => vec![],
        Op::Jump
        | Op::JumpTrue
        | Op::JumpFalse
        | Op::Return
        | Op::CapCheck
        | Op::CapRevoke
        | Op::Throw
        | Op::TryBegin
        | Op::TryEnd
        | Op::Safepoint
        | Op::Nop
        | Op::Halt => vec![],
    }
}
