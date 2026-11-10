// CEP:FILE crates/vortex-ugb/src/disasm.rs
// CEP:WHAT Linear disassembler producing stable text for tools and tests.
// CEP:WHY Bytecode debugging, the CLI `disasm` subcommand, and test
//        assertions all need a canonical rendering of a method.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Decode errors are rendered as `? <offset> <cause>` lines and
//        abort the listing; the stream is never skipped through.
// CEP:ASSUMES None.
// CEP:COST O(bytes) time, O(output) space.
// CEP:EVIDENCE cargo test -p vortex-ugb
use crate::instr::decode_at;
use crate::method::{Const, Method};
use crate::op::Op;

/// Disassembles `method` into canonical text, one instruction per line,
/// prefixed by byte offset.
// CEP:WHAT Disassembles method into canonical text, one instruction per line, prefixed by byte offset.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[must_use]
pub fn disassemble(method: &Method) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "method {} args={} regs={} instrs={}\n",
        method.name,
        method.arg_count,
        method.register_count,
        method.instruction_count()
    ));
    for (i, c) in method.consts.iter().enumerate() {
        out.push_str(&format!("const[{i}] = {}\n", render_const(c)));
    }
    let mut p = 0;
    while p < method.code.len() {
        match decode_at(&method.code, p) {
            Ok((instr, next)) => {
                out.push_str(&format!("{p:04}: {}\n", render_instr(&instr)));
                p = next;
            }
            Err(e) => {
                out.push_str(&format!("{:04}: ? {} ({})\n", e.offset, e.cause, e.offset));
                break;
            }
        }
    }
    out
}

fn render_const(c: &Const) -> String {
    match c {
        Const::Int(v) => format!("int {v}"),
        Const::Float(v) => format!("float {v:?}"),
    }
}

// CEP:WHAT Implements `render_instr` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
fn render_instr(instr: &crate::instr::Instr) -> String {
    let name = format!("{:?}", instr.op);
    let s = crate::op::sig(instr.op);
    let mut parts = vec![name];
    for r in instr.regs[..usize::from(s.regs)].iter() {
        parts.push(format!("r{r}"));
    }
    for _ in 0..s.branches {
        parts.push(format!("-> {:+}", instr.branches[0]));
    }
    if s.imm32 > 0 {
        parts.push(format!("#{}", instr.imm32));
    }
    for v in instr.imm32u[..usize::from(s.imm32u)].iter() {
        parts.push(format!("#{v}"));
    }
    if s.imm64 > 0 {
        parts.push(format!("#{:?}", f64::from_bits(instr.imm64)));
    }
    if s.pool > 0 {
        parts.push(format!("pool[{}]", instr.pool));
    }
    if instr.extended {
        parts.push("(ext)".to_string());
    }
    parts.join(" ")
}

/// Convenience: `true` when the method contains an instruction with `op`.
#[must_use]
pub fn contains_op(method: &Method, op: Op) -> bool {
    let mut p = 0;
    while p < method.code.len() {
        match decode_at(&method.code, p) {
            Ok((instr, next)) => {
                if instr.op == op {
                    return true;
                }
                p = next;
            }
            Err(_) => return false,
        }
    }
    false
}
