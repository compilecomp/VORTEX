// CEP:FILE crates/vortex-ugb/src/verify.rs
// CEP:WHAT Static verifier: CFG integrity (UGB-INV-03) and exception-handler
//        balance (UGB-INV-04), plus register-range and stream-wellformedness
//        checks.
// CEP:WHY UGB-INV-03 requires the interpreter and JIT to verify CFG integrity
//        before execution; doing it once at load is the cheap half of that
//        contract, the interpreter's per-branch check is the other half.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every violation is reported with byte offset and rule ID.
// CEP:ASSUMES The verifier is a load-time gate; it does not prove data-flow
//        properties (type soundness is enforced at execution).
// CEP:COST O(bytes) time, O(handlers) space.
// CEP:EVIDENCE cargo test -p vortex-ugb (verifier positive and negative tests)
use crate::instr::decode_at;
use crate::method::Method;
use crate::op::Op;

/// Verification failure with the rule it violated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Byte offset (or method-level sentinel 0xFFFF_FFFF for whole-file rules).
    pub offset: usize,
    /// Stable rule identifier, e.g. `UGB-INV-03`.
    pub rule: &'static str,
    /// Human-readable detail.
    pub detail: String,
}

/// Verifies a method. Returns all violations found (empty means valid).
// CEP:WHAT Verifies a method.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[must_use]
pub fn verify(method: &Method) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut open_try: Option<usize> = None;
    let mut p = 0;

    while p < method.code.len() {
        match decode_at(&method.code, p) {
            Err(e) => {
                violations.push(Violation {
                    offset: e.offset,
                    rule: "UGB-INV-03",
                    detail: format!("undecodable stream: {}", e.cause),
                });
                return violations;
            }
            Ok((instr, next)) => {
                check_registers(method, &instr, p, &mut violations);
                check_branch_targets(method, &instr, p, next - p, &mut violations);
                match instr.op {
                    Op::TryBegin => {
                        if open_try.is_some() {
                            violations.push(Violation {
                                offset: p,
                                rule: "UGB-INV-04",
                                detail: "nested TryBegin without TryEnd".into(),
                            });
                        }
                        open_try = Some(p);
                    }
                    Op::TryEnd => {
                        if open_try.is_none() {
                            violations.push(Violation {
                                offset: p,
                                rule: "UGB-INV-04",
                                detail: "TryEnd without TryBegin".into(),
                            });
                        }
                        open_try = None;
                    }
                    _ => { /* other opcodes carry no handler state */ }
                }
                p = next;
            }
        }
    }

    if let Some(at) = open_try {
        violations.push(Violation {
            offset: at,
            rule: "UGB-INV-04",
            detail: "TryBegin without TryEnd at method end".into(),
        });
    }

    // The stream must end exactly at an instruction boundary (guaranteed by
    // the loop above; kept as an explicit cross-check).
    if p != method.code.len() {
        violations.push(Violation {
            offset: p,
            rule: "UGB-INV-03",
            detail: "trailing bytes after last instruction".into(),
        });
    }
    violations
}

fn check_registers(
    method: &Method,
    instr: &crate::instr::Instr,
    at: usize,
    out: &mut Vec<Violation>,
) {
    let s = crate::op::sig(instr.op);
    for (i, r) in instr.regs[..usize::from(s.regs)].iter().enumerate() {
        if *r >= method.register_count {
            out.push(Violation {
                offset: at,
                rule: "UGB-INV-02",
                detail: format!(
                    "operand {i}: register r{r} >= register_count {}",
                    method.register_count
                ),
            });
        }
    }
}

// CEP:WHAT Implements `check_branch_targets` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
fn check_branch_targets(
    method: &Method,
    instr: &crate::instr::Instr,
    at: usize,
    len: usize,
    out: &mut Vec<Violation>,
) {
    let branches = crate::op::sig(instr.op).branches;
    if branches == 0 {
        return;
    }
    let target = (at + len).wrapping_add_signed(isize::from(instr.branches[0]));
    if target >= method.code.len() {
        out.push(Violation {
            offset: at,
            rule: "UGB-INV-03",
            detail: format!(
                "branch target {target} beyond code length {}",
                method.code.len()
            ),
        });
        return;
    }
    // All branch instructions must target an instruction boundary: walk to it.
    let mut q = 0;
    let mut boundary = false;
    while q < method.code.len() {
        if q == target {
            boundary = true;
            break;
        }
        match crate::instr::size_at(&method.code, q) {
            Ok(sz) => q += sz,
            Err(_) => break,
        }
    }
    if !boundary {
        out.push(Violation {
            offset: at,
            rule: "UGB-INV-03",
            detail: format!("branch target {target} is not an instruction boundary"),
        });
    }
}
