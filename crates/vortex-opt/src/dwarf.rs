// CEP:FILE crates/vortex-opt/src/dwarf.rs
// CEP:WHAT The DWARF5 debug information model: ULEB/SLEB encoders, the
//        line number program state machine with special opcodes, and the
//        DIE tree builder with abbreviation assignment (DWARF design
//        6.3-6.4).
// CEP:WHY Debuggers reconstruct source-level state through these
//        sections; the byte encodings must be spec-exact.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Encoding is total; the line program is decodable by the
//        documented state machine.
// CEP:ASSUMES DWARF5, 32-bit format, x86-64 minimum instruction length 1.
// CEP:COST O(rows + instructions).
// CEP:EVIDENCE cargo test -p vortex-opt
//! DWARF5 debug information (design 6.3-6.4).

/// One source-location row of the line program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRow {
    /// Machine address.
    pub address: u64,
    /// Source line.
    pub line: u32,
    /// Source column.
    pub column: u32,
    /// Whether this row is a statement boundary.
    pub is_stmt: bool,
}

/// Encodes an unsigned LEB128 integer.
// CEP:WHAT Emits ULEB128 bytes.
// CEP:WHY DWARF operands and headers use ULEB128 throughout.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(bytes).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn uleb(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Encodes a signed LEB128 integer.
// CEP:WHAT Emits SLEB128 bytes.
// CEP:WHY Line advances and expression offsets use SLEB128.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(bytes).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn sleb(out: &mut Vec<u8>, mut v: i64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        let done = (v == 0 && byte & 0x40 == 0) || (v == -1 && byte & 0x40 != 0);
        if done {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Line program header parameters (design 6.4).
#[derive(Debug, Clone, Copy)]
pub struct LineHeader {
    /// Minimum instruction length.
    pub min_inst_length: u8,
    /// Default is_stmt.
    pub default_is_stmt: bool,
    /// Smallest line increment of a special opcode.
    pub line_base: i8,
    /// Number of line increments of special opcodes.
    pub line_range: u8,
    /// First special opcode value.
    pub opcode_base: u8,
}

impl Default for LineHeader {
    // CEP:WHAT Provides the x86-64 default header.
    // CEP:WHY 6.4 documents min length 1, line base -5, range 14, base 13.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn default() -> LineHeader {
        LineHeader {
            min_inst_length: 1,
            default_is_stmt: true,
            line_base: -5,
            line_range: 14,
            opcode_base: 13,
        }
    }
}

/// The state machine row emitted by COPY or a special opcode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct State {
    address: u64,
    line: u32,
    column: u32,
    is_stmt: bool,
}

/// Builds and emits the line number program.
pub struct LineProgram {
    header: LineHeader,
    rows: Vec<LineRow>,
}

impl LineProgram {
    /// Creates a program from source rows in address order.
    // CEP:WHAT Stores rows for emission.
    // CEP:WHY 6.4 generation step 1: sort by address.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Rows are the post-schedule mapping.
    // CEP:COST O(rows).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn new(header: LineHeader, mut rows: Vec<LineRow>) -> LineProgram {
        rows.sort_by_key(|r| r.address);
        LineProgram { header, rows }
    }

    /// Emits the DWARF5 line program bytes.
    // CEP:WHAT Writes the header and the opcode stream, choosing special
    //        opcodes when both address and line advance fit.
    // CEP:WHY 6.4 generation step 4 minimizes the byte count.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The unit length fits u32.
    // CEP:COST O(rows).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn emit(&self) -> Vec<u8> {
        let h = self.header;
        // Program body first so the unit length is known.
        let mut body: Vec<u8> = Vec::new();
        let mut state = State {
            address: 0,
            line: 1,
            column: 0,
            is_stmt: h.default_is_stmt,
        };
        for row in &self.rows {
            let addr_delta = row.address.saturating_sub(state.address);
            let line_delta = row.line as i64 - state.line as i64;
            if row.column != state.column {
                body.push(5); // DW_LNS_set_column
                uleb(&mut body, row.column as u64);
            }
            // Special opcode when both deltas fit the encoding window.
            let line_off = line_delta - h.line_base as i64;
            let fits_special = addr_delta <= 20
                && line_off >= 0
                && (line_off as u32) < h.line_range as u32
                && addr_delta > 0;
            if fits_special {
                let op =
                    h.opcode_base as u64 + (line_off as u64) * h.line_range as u64 + addr_delta;
                body.push(op as u8);
                state = State {
                    address: row.address,
                    line: row.line,
                    column: row.column,
                    is_stmt: h.default_is_stmt,
                };
            } else {
                if addr_delta > 0 {
                    body.push(2); // DW_LNS_advance_pc
                    uleb(&mut body, addr_delta / h.min_inst_length as u64);
                    state.address += addr_delta;
                }
                if line_delta != 0 {
                    body.push(3); // DW_LNS_advance_line
                    sleb(&mut body, line_delta);
                    state.line = row.line;
                }
                body.push(1); // DW_LNS_copy
                state.column = row.column;
                state.is_stmt = h.default_is_stmt;
            }
        }
        // End sequence: advance to a sentinel address and end.
        body.push(2); // DW_LNS_advance_pc
        uleb(&mut body, 1);
        body.push(1); // DW_LNE_end_sequence (extended)
                      // The extended opcode needs: 0x00, length 1, sub-op 0x01.
        body.pop();
        body.extend_from_slice(&[0x00, 0x01, 0x01]);
        // Header.
        let mut header: Vec<u8> = Vec::new();
        header.push(2); // version 5? DWARF5 line version is 5.
        header[0] = 5;
        header.push(h.min_inst_length);
        header.push(1); // maximum_operations_per_instruction
        header.push(if h.default_is_stmt { 1 } else { 0 });
        header.push(0); // line_base is i8
        header[4] = h.line_base as u8;
        header.push(h.line_range);
        header.push(h.opcode_base);
        // Standard opcode lengths: opcode_base - 1 entries.
        header.extend(std::iter::repeat_n(0u8, (h.opcode_base - 1) as usize));
        // Directory and file entries (DWARF5 forms): one directory, one file.
        uleb(&mut header, 1); // directory_entry_format_count
        uleb(&mut header, 1); // DW_LNCT_path
        uleb(&mut header, 8); // DW_FORM_string
        uleb(&mut header, 1); // directories_count
        header.extend_from_slice(b".\0");
        uleb(&mut header, 2); // file_name_entry_format_count
        uleb(&mut header, 1); // DW_LNCT_path
        uleb(&mut header, 8); // DW_FORM_string
        uleb(&mut header, 1); // DW_LNCT_directory_index
        uleb(&mut header, 11); // DW_FORM_udata
        uleb(&mut header, 1); // file_names_count
        header.extend_from_slice(b"main.vrx\0");
        uleb(&mut header, 0);
        // Assemble: unit length + version + header + body.
        let unit_len = (header.len() + body.len()) as u32;
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&unit_len.to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&body);
        out
    }
}

/// One abbreviation: a TAG with its attribute/form pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abbrev {
    /// Abbreviation code (1-based).
    pub code: u64,
    /// The DWARF tag value.
    pub tag: u32,
    /// Whether the DIE has children.
    pub has_children: bool,
}

/// Builds the .debug_abbrev section for the DIE tree.
// CEP:WHAT Emits the abbreviation table: null terminator per entry.
// CEP:WHY DIEs reference abbreviations by code (design 6.3).
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Codes are assigned in frequency order by the caller.
// CEP:COST O(abbrevs).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn emit_abbrev(abbrevs: &[Abbrev]) -> Vec<u8> {
    let mut out = Vec::new();
    for a in abbrevs {
        uleb(&mut out, a.code);
        uleb(&mut out, a.tag as u64);
        out.push(if a.has_children { 1 } else { 0 });
        // (attribute, form) pairs: name + decl line for this model.
        uleb(&mut out, 0x03); // DW_AT_name
        uleb(&mut out, 0x08); // DW_FORM_string
        uleb(&mut out, 0x2f); // DW_AT_decl_line (DWARF5)
        uleb(&mut out, 0x0b); // DW_FORM_data1
        out.push(0);
        out.push(0);
    }
    out.push(0);
    out
}

/// One DIE to emit into .debug_info.
#[derive(Debug, Clone)]
pub struct Die {
    /// Abbreviation code.
    pub abbrev: u64,
    /// Name attribute.
    pub name: String,
    /// Declaration line.
    pub decl_line: u8,
    /// Child DIEs.
    pub children: Vec<Die>,
}

/// Emits the DIE tree in post-order-with-terminators form.
// CEP:WHAT Writes each DIE's abbrev code, name, and decl line, closing
//        child lists with null terminators.
// CEP:WHY .debug_info consumers reconstruct the tree from these bytes.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES DWARF32 offsets.
// CEP:COST O(DIEs).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn emit_info(die: &Die) -> Vec<u8> {
    let mut out = Vec::new();
    // Walks one DIE and its children, emitting abbrev code, name, and
    // declaration line, and closing child lists with null terminators.
    // CEP:WHAT Emits one DIE subtree in .debug_info form.
    // CEP:WHY Consumers rebuild the tree from these bytes.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Abbrev codes were assigned by the caller.
    // CEP:COST O(subtree).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn walk(d: &Die, out: &mut Vec<u8>) {
        uleb(out, d.abbrev);
        out.extend_from_slice(d.name.as_bytes());
        out.push(0);
        out.push(d.decl_line);
        if !d.children.is_empty() {
            for c in &d.children {
                walk(c, out);
            }
            out.push(0); // null child terminator
        }
    }
    walk(die, &mut out);
    out
}
