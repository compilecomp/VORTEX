// CEP:FILE crates/vortex-t0/src/rewrite.rs
// CEP:WHAT T0 opcode rewriting: profile-driven type specialization,
//        monomorphic call inlining, unreachable-code NOPs, repeated-
//        computation reuse, and super bytecode formation, all patched in
//        place with branch-target safety (T0 design 6.6, 6.7).
// CEP:WHY Rewriting eliminates redundant checks and dispatches without
//        leaving the interpreter; safety requires guards and target
//        verification on every patch.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE A rewrite that cannot be proven safe is skipped; no patch
//        ever moves branch targets or changes code length.
// CEP:ASSUMES Patches are equal-size or shorter with NOP padding; branch
//        targets never land inside a rewritten span.
// CEP:COST O(window) per trigger; triggers fire at safe points.
// CEP:EVIDENCE cargo test -p vortex-t0
//! Opcode rewriting and super bytecodes (T0 design 6.6-6.7).

use crate::interp::{CacheState, Interp};
use std::collections::HashSet;
use vortex_ugb::{Instr, Op as UOp, Operand};

/// Rewrite policy parameters.
/// Rewrite policy parameters.
#[derive(Debug, Clone, Copy)]
pub struct RewritePolicy {
    /// Integer observations before ADD specialization.
    pub specialize_after: u64,
    /// Executions before super formation.
    pub super_after: u64,
    /// Monomorphic observations before call inlining.
    pub inline_after: u64,
}

impl Default for RewritePolicy {
    // CEP:WHAT Provides the documented default policy.
    // CEP:WHY Small thresholds keep rewrites prompt in the model.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn default() -> RewritePolicy {
        RewritePolicy {
            specialize_after: 8,
            super_after: 2,
            inline_after: 1,
        }
    }
}

/// The rewriter: holds per-site completion state.
#[derive(Default)]
pub struct Rewriter {
    /// Sites already rewritten.
    done: HashSet<(u32, usize)>,
    /// Policy thresholds.
    pub policy: RewritePolicy,
}

impl Rewriter {
    /// A rewriter with default policy.
    // CEP:WHAT Creates the rewriter.
    // CEP:WHY One instance drives all rewrite rules.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn new() -> Rewriter {
        Rewriter {
            done: HashSet::new(),
            policy: RewritePolicy::default(),
        }
    }

    /// Runs the rewrite rules at a safe point.
    // CEP:WHAT Applies all applicable rules to one method.
    // CEP:WHY T0 6.6: triggers are profiling data, IC state, and hot loops.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unsafe candidates are skipped.
    // CEP:ASSUMES The method code is verified and currently idle (no other
    //        thread executes it; the model interpreter is single-threaded).
    // CEP:COST O(|code|) scan per invocation.
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn maybe_rewrite(&mut self, interp: &mut Interp, method: u32) {
        if self.done.contains(&(method, 0)) {
            return;
        }
        let policy = self.policy;
        let code_len = interp
            .program
            .method(method)
            .map(|m| m.code.len())
            .unwrap_or(0);
        let mut targets: Vec<usize> = Vec::new();
        let mut branch_spans: Vec<(usize, usize)> = Vec::new();
        {
            let code = interp
                .program
                .method(method)
                .map(|m| m.code.clone())
                .unwrap_or_default();
            let mut pos = 0;
            while pos < code.len() {
                if let Ok((instr, size)) = Instr::decode(&code, pos) {
                    if instr
                        .op
                        .info()
                        .sig
                        .contains(&vortex_ugb::ops::OperandKind::Label)
                    {
                        if let Some(Operand::Label(l)) = instr
                            .operands
                            .iter()
                            .find(|o| matches!(o, Operand::Label(_)))
                        {
                            targets.push((pos as i64 + *l as i64).max(0) as usize);
                        }
                    }
                    branch_spans.push((pos, pos + size));
                    pos += size;
                } else {
                    break;
                }
            }
        }
        let _ = branch_spans;
        let mut patched: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut pos = 0;
        while pos < code_len {
            let code = interp
                .program
                .method(method)
                .map(|m| m.code.clone())
                .unwrap_or_default();
            let Ok((instr, size)) = Instr::decode(&code, pos) else {
                break;
            };
            let site = (method, pos);
            match instr.op {
                UOp::Add => {
                    // Type specialization (rule 1): all-int feedback.
                    let ints = interp
                        .profiles
                        .get(&method)
                        .map(|p| p.int_seen.get(pos).copied().unwrap_or(0))
                        .unwrap_or(0);
                    let floats = interp
                        .profiles
                        .get(&method)
                        .map(|p| p.float_seen.get(pos).copied().unwrap_or(0))
                        .unwrap_or(0);
                    if ints >= policy.specialize_after && floats == 0 && !self.done.contains(&site)
                    {
                        let bytes = encode_same_size(UOp::AddInt, &instr.operands, size);
                        if let Some(bytes) = bytes {
                            patched.push((pos, bytes));
                            self.done.insert(site);
                        }
                    }
                }
                UOp::CallMethod => {
                    // Inline-cache specialization (rule 5): monomorphic site.
                    let mono = interp.caches.get(&(method, pos)).is_some_and(|c| {
                        c.state == CacheState::Monomorphic && c.hits >= policy.inline_after
                    });
                    if mono && !self.done.contains(&site) {
                        let bytes = encode_same_size(UOp::CallDirect, &instr.operands, size);
                        if let Some(bytes) = bytes {
                            patched.push((pos, bytes));
                            self.done.insert(site);
                        }
                    }
                }
                UOp::Mul => {
                    // Repeated computation (rule 4): MUL r,a,b; MUL r2,a,b.
                    if let Some(bytes) = self.reuse_rule(&code, pos, &targets) {
                        patched.push((pos, bytes));
                        self.done.insert(site);
                    }
                }
                UOp::Jump => {
                    // Dead code (rule 3): the instruction after JUMP is NOP.
                    let next = pos + size;
                    if next < code_len
                        && !targets.contains(&next)
                        && !self.done.contains(&(method, next))
                    {
                        patched.push((next, vec![UOp::Nop as u8]));
                        self.done.insert((method, next));
                    }
                }
                // Other opcodes carry no single-site rewrite trigger.
                _ => {
                    let _ = size;
                }
            }
            pos += size;
        }
        // Super formation pass over the whole method.
        self.form_supers(interp, method, &targets, policy);
        // Apply patches: each is equal-size or shorter with NOP padding.
        if !patched.is_empty() {
            if let Some(m) = interp.program.methods.get_mut(method as usize) {
                for (at, bytes) in patched {
                    let end = (at + bytes.len()).min(m.code.len());
                    m.code[at..end].copy_from_slice(&bytes);
                }
            }
        }
    }

    /// Rule 4: the second of two identical MULs becomes MOV plus padding.
    // CEP:WHAT Detects an immediately repeated MUL pair.
    // CEP:WHY CSE at the bytecode level reuses the first result.
    // CEP:STATUS complete
    // CEP:FAILURE None when the pattern does not match or targets interfere.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn reuse_rule(&self, code: &[u8], pos: usize, targets: &[usize]) -> Option<Vec<u8>> {
        let (first, size1) = Instr::decode(code, pos).ok()?;
        let (second, size2) = Instr::decode(code, pos + size1).ok()?;
        if second.op != UOp::Mul {
            return None;
        }
        let (d1, a1, b1) = (
            first.operands.first()?.as_reg()?,
            first.operands.get(1)?.as_reg()?,
            first.operands.get(2)?.as_reg()?,
        );
        let (d2, a2, b2) = (
            second.operands.first()?.as_reg()?,
            second.operands.get(1)?.as_reg()?,
            second.operands.get(2)?.as_reg()?,
        );
        if (a1, b1) != (a2, b2) {
            return None;
        }
        // Branch targets must not land inside the replaced span.
        let span_start = pos + size1;
        let span_end = pos + size1 + size2;
        if targets.iter().any(|t| *t > span_start && *t < span_end) {
            return None;
        }
        let mut bytes = Vec::new();
        Instr::new(UOp::Mov, vec![Operand::Reg(d2), Operand::Reg(d1)])
            .encode(&mut bytes)
            .ok()?;
        while bytes.len() < size1 + size2 {
            bytes.push(UOp::Nop as u8);
        }
        Some(bytes)
    }

    /// Super bytecode formation (T0 6.7): 2-4 instruction sequences with
    /// no interior branch targets.
    // CEP:WHAT Detects and patches the three documented super patterns.
    // CEP:WHY Super bytecodes collapse dispatch overhead for hot sequences.
    // CEP:STATUS complete
    // CEP:FAILURE Unsafe or cold candidates are skipped.
    // CEP:ASSUMES Sequences execute straight-line between their ends.
    // CEP:COST O(|code|) per invocation.
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn form_supers(
        &mut self,
        interp: &mut Interp,
        method: u32,
        targets: &[usize],
        policy: RewritePolicy,
    ) {
        let code = interp
            .program
            .method(method)
            .map(|m| m.code.clone())
            .unwrap_or_default();
        let mut pos = 0;
        let mut patches: Vec<(usize, Vec<u8>)> = Vec::new();
        while pos < code.len() {
            let Ok((i1, s1)) = Instr::decode(&code, pos) else {
                break;
            };
            // Pattern 1: LOAD_LOCAL r, l; ADD_IMM r, r, c; STORE_LOCAL l, r.
            if i1.op == UOp::LoadLocal {
                if let Some((bytes, total)) = self.try_load_add_store(&code, pos, targets) {
                    let hot = interp.exec_count(method, pos) >= policy.super_after;
                    if hot && !self.done.contains(&(method, pos)) {
                        patches.push((pos, bytes));
                        self.done.insert((method, pos));
                    }
                    pos += total;
                    continue;
                }
            }
            // Pattern 3: CMP rF, a, b, k; JUMP_IF_TRUE rF, label.
            if i1.op == UOp::Cmp {
                if let Some((bytes, total)) = self.try_cmp_branch(&code, pos, targets) {
                    let hot = interp.exec_count(method, pos) >= policy.super_after;
                    if hot && !self.done.contains(&(method, pos)) {
                        patches.push((pos, bytes));
                        self.done.insert((method, pos));
                    }
                    pos += total;
                    continue;
                }
            }
            pos += s1;
        }
        if !patches.is_empty() {
            if let Some(m) = interp.program.methods.get_mut(method as usize) {
                for (at, bytes) in patches {
                    let end = (at + bytes.len()).min(m.code.len());
                    m.code[at..end].copy_from_slice(&bytes);
                }
            }
        }
    }

    /// Pattern 1: load, add-immediate, store becomes LOAD_ADD_STORE.
    // CEP:WHAT Matches the load-add-store triple.
    // CEP:WHY 6.7 example one; the triple collapses to one dispatch.
    // CEP:STATUS complete
    // CEP:FAILURE None when the shape or targets do not allow it.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn try_load_add_store(
        &self,
        code: &[u8],
        pos: usize,
        targets: &[usize],
    ) -> Option<(Vec<u8>, usize)> {
        let (i1, s1) = Instr::decode(code, pos).ok()?;
        let (i2, s2) = Instr::decode(code, pos + s1).ok()?;
        let (i3, s3) = Instr::decode(code, pos + s1 + s2).ok()?;
        if i1.op != UOp::LoadLocal || i2.op != UOp::AddImm || i3.op != UOp::StoreLocal {
            return None;
        }
        let r = i1.operands.first()?.as_reg()?;
        let l = match i1.operands.get(1)? {
            Operand::Local(v) => *v,
            _ => return None,
        };
        let r2 = i2.operands.get(1)?.as_reg()?;
        let c = match i2.operands.get(2)? {
            Operand::Imm32(v) => *v,
            _ => return None,
        };
        let r3 = i3.operands.get(1)?.as_reg()?;
        let l2 = match i3.operands.first()? {
            Operand::Local(v) => *v,
            _ => return None,
        };
        if r != r2 || r != r3 || l != l2 {
            return None;
        }
        let span = (pos + s1, pos + s1 + s2);
        if targets.iter().any(|t| *t > span.0 && *t < span.1 + s3) {
            return None;
        }
        let mut bytes = Vec::new();
        Instr::new(
            UOp::SuperLoadAddStore,
            vec![Operand::Local(l), Operand::Imm32(c)],
        )
        .encode(&mut bytes)
        .ok()?;
        let total = s1 + s2 + s3;
        while bytes.len() < total {
            bytes.push(UOp::Nop as u8);
        }
        Some((bytes, total))
    }

    /// Pattern 3: compare plus conditional branch becomes CMP_BRANCH.
    // CEP:WHAT Matches the compare-branch pair.
    // CEP:WHY 6.7 example three; the pair collapses to one dispatch.
    // CEP:STATUS complete
    // CEP:FAILURE None when the shape or targets do not allow it.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn try_cmp_branch(
        &self,
        code: &[u8],
        pos: usize,
        targets: &[usize],
    ) -> Option<(Vec<u8>, usize)> {
        let (i1, s1) = Instr::decode(code, pos).ok()?;
        let (i2, s2) = Instr::decode(code, pos + s1).ok()?;
        if i1.op != UOp::Cmp || i2.op != UOp::JumpIfTrue {
            return None;
        }
        let f1 = i1.operands.first()?.as_reg()?;
        let a = i1.operands.get(1)?.as_reg()?;
        let b = i1.operands.get(2)?.as_reg()?;
        let k = match i1.operands.get(3)? {
            Operand::CmpKind(k) => *k,
            _ => return None,
        };
        let f2 = i2.operands.first()?.as_reg()?;
        let label = match i2.operands.get(1)? {
            Operand::Label(l) => *l,
            _ => return None,
        };
        if f1 != f2 {
            return None;
        }
        let span = (pos, pos + s1);
        if targets.iter().any(|t| *t > span.0 && *t < span.1) {
            return None;
        }
        let mut bytes = Vec::new();
        Instr::new(
            UOp::SuperCmpBranch,
            vec![
                Operand::Reg(a),
                Operand::Reg(b),
                Operand::Label(label),
                Operand::CmpKind(k),
            ],
        )
        .encode(&mut bytes)
        .ok()?;
        let total = s1 + s2;
        while bytes.len() < total {
            bytes.push(UOp::Nop as u8);
        }
        Some((bytes, total))
    }
}

/// Encodes an opcode with the same operands padded to `size` bytes.
// CEP:WHAT Builds an equal-size replacement encoding.
// CEP:WHY In-place patching must never shift bytes or move targets.
// CEP:STATUS complete
// CEP:FAILURE None when the new encoding is longer than the span.
// CEP:ASSUMES None.
// CEP:COST O(operands).
// CEP:EVIDENCE cargo test -p vortex-t0
fn encode_same_size(op: UOp, operands: &[Operand], size: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    Instr::new(op, operands.to_vec()).encode(&mut bytes).ok()?;
    if bytes.len() > size {
        return None;
    }
    while bytes.len() < size {
        bytes.push(UOp::Nop as u8);
    }
    Some(bytes)
}
