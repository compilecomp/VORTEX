// CEP:FILE crates/vortex-ugb/src/asm.rs
// CEP:WHAT Text assembly front end: parse UGB assembly into instruction
//        bytes with two-pass label resolution, and render bytes to text.
// CEP:WHY Front-end authors and the CLI demo need a readable source form;
//        the disassembler is its inverse for round-trip checks.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE AsmError names the line and the problem; parsing never
//        panics and never patches outside the emitted buffer.
// CEP:ASSUMES Registers are rN, labels are .name, one instruction per
//        line, comma-separated operands.
// CEP:COST O(source + code).
// CEP:EVIDENCE cargo test -p vortex-ugb
//! Text assembly and disassembly.

use crate::instr::{CodecError, Instr, Operand};
use crate::method::Method;
use crate::ops::{CmpKind, Op, OperandKind};
use std::collections::BTreeMap;
use std::fmt;

/// Assembly failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsmError {
    /// The mnemonic is not defined.
    UnknownMnemonic {
        /// 1-based source line.
        line: usize,
        /// The mnemonic text.
        text: String,
    },
    /// The operand count disagrees with the opcode signature.
    OperandCount {
        /// 1-based source line.
        line: usize,
        /// Expected count.
        expected: usize,
        /// Found count.
        found: usize,
    },
    /// An operand did not parse as its signature kind.
    BadOperand {
        /// 1-based source line.
        line: usize,
        /// The operand text.
        text: String,
        /// Expected kind name.
        expected: &'static str,
    },
    /// A branch label was never defined.
    UndefinedLabel {
        /// 1-based source line of the branch.
        line: usize,
        /// The label name.
        label: String,
    },
    /// The instruction failed to encode.
    Encode {
        /// 1-based source line.
        line: usize,
        /// Underlying codec error.
        error: CodecError,
    },
    /// The label offset arithmetic left the i32 operand domain.
    LabelOverflow {
        /// 1-based source line of the branch.
        line: usize,
    },
}

impl fmt::Display for AsmError {
    // CEP:WHAT Renders an assembly failure with its source line.
    // CEP:WHY Authors need the failing line without a debugger.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsmError::UnknownMnemonic { line, text } => {
                write!(f, "line {line}: unknown mnemonic {text}")
            }
            AsmError::OperandCount {
                line,
                expected,
                found,
            } => {
                write!(
                    f,
                    "line {line}: expected {expected} operands, found {found}"
                )
            }
            AsmError::BadOperand {
                line,
                text,
                expected,
            } => {
                write!(f, "line {line}: operand {text} is not a {expected}")
            }
            AsmError::UndefinedLabel { line, label } => {
                write!(f, "line {line}: label {label} is never defined")
            }
            AsmError::Encode { line, error } => {
                write!(f, "line {line}: encode failed: {error}")
            }
            AsmError::LabelOverflow { line } => {
                write!(f, "line {line}: branch offset exceeds the i32 label domain")
            }
        }
    }
}

impl std::error::Error for AsmError {}

/// Byte offset of each operand relative to the instruction start.
// CEP:WHAT Computes operand byte offsets from the opcode signature.
// CEP:WHY Label patching needs the exact position of the 4 label bytes.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES The codec's widths: 1 byte for tags and selectors, 2 for
//        u16 operands, 4 for i32 and u32 operands, 8 for 64-bit operands.
// CEP:COST O(operands).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn operand_offsets(op: Op) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut pos = 1;
    for kind in op.info().sig {
        offsets.push(pos);
        pos += match kind {
            OperandKind::TypeTag | OperandKind::CmpKind | OperandKind::LangId => 1,
            OperandKind::Reg | OperandKind::CapId | OperandKind::Count | OperandKind::Local => 2,
            OperandKind::Imm32
            | OperandKind::Label
            | OperandKind::FfiDesc
            | OperandKind::ConstIdx => 4,
            OperandKind::Imm64 | OperandKind::F64 => 8,
        };
    }
    offsets
}

/// A parsed source line: mnemonic plus raw operand tokens.
struct Line {
    mnemonic: String,
    tokens: Vec<String>,
    label_def: Option<String>,
}

/// Splits one source line into a label definition and operand tokens.
// CEP:WHAT Parses one assembly line's shape.
// CEP:WHY Two-pass assembly needs label definitions separated from code.
// CEP:STATUS complete
// CEP:FAILURE Comments and blank lines yield a line with no mnemonic.
// CEP:ASSUMES One instruction per line; labels end with a colon.
// CEP:COST O(line length).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn split_line(raw: &str) -> Line {
    let text = raw.split("//").next().unwrap_or("").trim();
    let mut label_def = None;
    let mut rest = text;
    if let Some(idx) = text.find(':') {
        let head = text[..idx].trim();
        if let Some(name) = head.strip_prefix('.') {
            label_def = Some(name.to_string());
            rest = text[idx + 1..].trim();
        }
    }
    let mut parts = rest.split_whitespace();
    let mnemonic = parts.next().unwrap_or("").to_string();
    let tokens = parts
        .collect::<Vec<&str>>()
        .join(" ")
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    Line {
        mnemonic,
        tokens,
        label_def,
    }
}

/// Parses a register token.
// CEP:WHAT Parses rN into a register index.
// CEP:WHY Register operands are the most common form.
// CEP:STATUS complete
// CEP:FAILURE None for malformed tokens.
// CEP:ASSUMES Lowercase r prefix.
// CEP:COST O(token).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn parse_reg(t: &str) -> Option<u16> {
    let n = t.strip_prefix('r')?;
    n.parse::<u16>().ok()
}

/// Parses an operand token by its signature kind.
// CEP:WHAT Converts one token into its operand value.
// CEP:WHY Signature-driven parsing keeps the assembler in sync with ops.
// CEP:STATUS complete
// CEP:FAILURE None when the token does not match the kind.
// CEP:ASSUMES Numeric literals are decimal with optional sign; floats end
//        with D; 64-bit integers end with L; labels are names here.
// CEP:COST O(token).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn parse_operand(kind: &OperandKind, t: &str) -> Option<Operand> {
    match kind {
        OperandKind::Reg => parse_reg(t).map(Operand::Reg),
        OperandKind::Imm32 => t.parse::<i32>().ok().map(Operand::Imm32),
        OperandKind::Imm64 => t
            .strip_suffix('L')
            .and_then(|s| s.parse::<i64>().ok())
            .map(Operand::Imm64),
        OperandKind::F64 => t
            .strip_suffix('D')
            .and_then(|s| s.parse::<f64>().ok())
            .map(Operand::F64),
        OperandKind::CapId => t
            .strip_prefix("cap")
            .and_then(|s| s.parse::<u16>().ok())
            .map(Operand::CapId),
        OperandKind::Label => parse_label(t).map(Operand::Label),
        OperandKind::TypeTag => t
            .strip_prefix("type")
            .and_then(|s| s.parse::<u8>().ok())
            .map(Operand::TypeTag),
        OperandKind::FfiDesc => t
            .strip_prefix("fd")
            .and_then(|s| s.parse::<u32>().ok())
            .map(Operand::FfiDesc),
        OperandKind::CmpKind => parse_cmp(t).map(Operand::CmpKind),
        OperandKind::Count => t
            .strip_prefix('x')
            .and_then(|s| s.parse::<u16>().ok())
            .map(Operand::Count),
        OperandKind::Local => t
            .strip_prefix("local")
            .and_then(|s| s.parse::<u16>().ok())
            .map(Operand::Local),
        OperandKind::ConstIdx => t
            .strip_prefix("const")
            .and_then(|s| s.parse::<u32>().ok())
            .map(Operand::ConstIdx),
        OperandKind::LangId => t
            .strip_prefix("lang")
            .and_then(|s| s.parse::<u8>().ok())
            .map(Operand::LangId),
    }
}

/// Parses a comparison mnemonic.
// CEP:WHAT Parses a comparison selector token.
// CEP:WHY CMP and CMP_BRANCH carry the kind as text in assembly.
// CEP:STATUS complete
// CEP:FAILURE None for unknown selectors.
// CEP:ASSUMES The rendering in Operand::render produces these names.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn parse_cmp(t: &str) -> Option<CmpKind> {
    match t {
        "Eq" => Some(CmpKind::Eq),
        "Ne" => Some(CmpKind::Ne),
        "Lt" => Some(CmpKind::Lt),
        "Le" => Some(CmpKind::Le),
        "Gt" => Some(CmpKind::Gt),
        "Ge" => Some(CmpKind::Ge),
        _ => None,
    }
}

/// Parses a label token: a named reference (handled by the caller) or a
/// resolved relative offset such as `+0x8` or `-0x4`.
// CEP:WHAT Parses resolved label offsets back into operands.
// CEP:WHY Disassembled output must re-assemble byte-identically; the
//        disassembler renders resolved offsets.
// CEP:STATUS complete
// CEP:FAILURE None for tokens that are neither names nor offsets.
// CEP:ASSUMES Offsets are decimal or 0x-prefixed hexadecimal with sign.
// CEP:COST O(token).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn parse_label(t: &str) -> Option<i32> {
    let (sign, body) = match t.strip_prefix('-') {
        Some(b) => (-1i64, b),
        None => (1i64, t.strip_prefix('+').unwrap_or(t)),
    };
    let magnitude = if let Some(hex) = body.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()?
    } else if body.starts_with('.') {
        return None;
    } else {
        body.parse::<i64>().ok()?
    };
    let value = sign * magnitude;
    if value < i32::MIN as i64 || value > i32::MAX as i64 {
        return None;
    }
    Some(value as i32)
}

/// Assembles UGB text into a method's byte stream.
// CEP:WHAT Two-pass assembly: emit with zero labels, then patch offsets.
// CEP:WHY Forward branches need byte offsets known only after emission.
// CEP:STATUS complete
// CEP:FAILURE AsmError with the failing source line.
// CEP:ASSUMES Labels are unique; branch offsets are relative to the
//        branch instruction's own byte offset.
// CEP:COST O(source + code).
// CEP:EVIDENCE cargo test -p vortex-ugb
// CEP:ID: IR-a08
pub fn assemble(builder: &mut Method, source: &str) -> Result<(), AsmError> {
    let lines: Vec<Line> = source.lines().map(split_line).collect();
    let mut label_offsets: BTreeMap<String, usize> = BTreeMap::new();
    let mut pending: Vec<(usize, usize, usize, String)> = Vec::new();
    let mut buffer: Vec<u8> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        if let Some(name) = &line.label_def {
            label_offsets.insert(name.clone(), buffer.len());
        }
        if line.mnemonic.is_empty() {
            continue;
        }
        let op = Op::all()
            .iter()
            .copied()
            .find(|o| o.info().name.eq_ignore_ascii_case(&line.mnemonic))
            .ok_or_else(|| AsmError::UnknownMnemonic {
                line: li + 1,
                text: line.mnemonic.clone(),
            })?;
        let info = op.info();
        if info.sig.len() != line.tokens.len() {
            return Err(AsmError::OperandCount {
                line: li + 1,
                expected: info.sig.len(),
                found: line.tokens.len(),
            });
        }
        let mut operands = Vec::with_capacity(info.sig.len());
        let instr_start = buffer.len();
        let offsets = operand_offsets(op);
        for (oi, (kind, token)) in info.sig.iter().zip(line.tokens.iter()).enumerate() {
            if *kind == OperandKind::Label && token.starts_with('.') {
                pending.push((
                    li + 1,
                    instr_start,
                    instr_start + offsets[oi],
                    token[1..].to_string(),
                ));
                operands.push(Operand::Label(0));
            } else {
                let operand = parse_operand(kind, token).ok_or_else(|| AsmError::BadOperand {
                    line: li + 1,
                    text: token.clone(),
                    expected: kind_name(kind),
                })?;
                operands.push(operand);
            }
        }
        Instr::new(op, operands)
            .encode(&mut buffer)
            .map_err(|e| AsmError::Encode {
                line: li + 1,
                error: e,
            })?;
    }
    for (line_no, instr_start, patch_at, name) in pending {
        let target = label_offsets
            .get(&name)
            .copied()
            .ok_or_else(|| AsmError::UndefinedLabel {
                line: line_no,
                label: name.clone(),
            })?;
        let delta = target as i64 - instr_start as i64;
        if delta < i32::MIN as i64 || delta > i32::MAX as i64 {
            return Err(AsmError::LabelOverflow { line: line_no });
        }
        buffer[patch_at..patch_at + 4].copy_from_slice(&(delta as i32).to_le_bytes());
    }
    builder.code = buffer;
    Ok(())
}

/// Static kind names for error messages.
// CEP:WHAT Names an operand kind for diagnostics.
// CEP:WHY Error messages must state the expected shape.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn kind_name(kind: &OperandKind) -> &'static str {
    match kind {
        OperandKind::Reg => "register",
        OperandKind::Imm32 => "i32 immediate",
        OperandKind::Imm64 => "i64 immediate",
        OperandKind::F64 => "f64 immediate",
        OperandKind::CapId => "capability",
        OperandKind::Label => "label",
        OperandKind::TypeTag => "type tag",
        OperandKind::FfiDesc => "ffi descriptor",
        OperandKind::CmpKind => "comparison",
        OperandKind::Count => "count",
        OperandKind::Local => "local",
        OperandKind::ConstIdx => "constant",
        OperandKind::LangId => "language",
    }
}

/// Disassembles a method's code stream to assembly text.
// CEP:WHAT Renders a byte stream back to text, one instruction per line.
// CEP:WHY Diagnostics, tests, and the CLI need readable code.
// CEP:STATUS complete
// CEP:FAILURE CodecError when the stream is malformed.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
pub fn disassemble(code: &[u8]) -> Result<String, CodecError> {
    let mut out = String::new();
    let mut pos = 0;
    while pos < code.len() {
        let (instr, size) = Instr::decode(code, pos)?;
        out.push_str(&render(&instr));
        out.push('\n');
        pos += size;
    }
    Ok(out)
}

/// Renders one instruction in assembly syntax.
// CEP:WHAT Formats one instruction with mnemonic and operands.
// CEP:WHY Round-trip tests compare rendered text with expected text.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(operands).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn render(instr: &Instr) -> String {
    let info = instr.op.info();
    let mut line = info.name.to_string();
    let rendered: Vec<String> = instr.operands.iter().map(|o| o.render()).collect();
    line.push(' ');
    line.push_str(&rendered.join(", "));
    line
}
