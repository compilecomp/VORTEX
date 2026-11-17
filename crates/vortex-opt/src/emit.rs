// CEP:FILE crates/vortex-opt/src/emit.rs
// CEP:WHAT The ELF64 object emitter: section and symbol models, relocation
//        generation from fixups, section layout with alignment, and the
//        full ELF64 header plus section header table serialization
//        (Code Emission design 6.3-6.9).
// CEP:WHY Machine code ships as objects; conformance to the format is a
//        validity invariant of the backend.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Layout failures surface as typed EmitError entries.
// CEP:ASSUMES Little-endian x86-64 class-64 ELF.
// CEP:COST O(|I| k + |R| + |S| + |L|).
// CEP:EVIDENCE cargo test -p vortex-opt
//! ELF64 object emission (design 6.3-6.9).

use std::fmt;

/// Emitter failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// A symbol was referenced but never defined.
    UndefinedSymbol {
        /// The symbol name.
        name: String,
    },
    /// Two symbols share a name.
    DuplicateSymbol {
        /// The symbol name.
        name: String,
    },
}

impl fmt::Display for EmitError {
    // CEP:WHAT Renders an emission failure.
    // CEP:WHY Diagnostics name the symbol at fault.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmitError::UndefinedSymbol { name } => write!(f, "undefined symbol {name}"),
            EmitError::DuplicateSymbol { name } => write!(f, "duplicate symbol {name}"),
        }
    }
}

impl std::error::Error for EmitError {}

/// Relocation types (design 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocType {
    /// 64-bit absolute: value = S + A.
    Absolute64,
    /// 32-bit PC-relative: value = S + A - P.
    Pc32,
    /// 32-bit PC-relative into the PLT: value = PLT(S) + A - P
    /// (R_X86_64_PLT32); binds directly to S when S is defined.
    Plt32,
    /// Dynamic jump-slot metadata for a GOT entry (R_X86_64_JUMP_SLOT).
    JumpSlot,
}

/// ELF section types used by the emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionType {
    /// Program contents.
    Progbits,
    /// Uninitialized space.
    Nobits,
    /// Symbol table.
    Symtab,
    /// String table.
    Strtab,
    /// Relocation entries with explicit addends.
    Rela,
}

/// ELF section flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionFlags {
    /// Occupies memory at runtime.
    pub alloc: bool,
    /// Writable.
    pub write: bool,
    /// Executable.
    pub exec: bool,
}

/// One output section (design 6.6).
#[derive(Debug, Clone)]
pub struct Section {
    /// Section name.
    pub name: String,
    /// ELF type.
    pub ty: SectionType,
    /// Flags.
    pub flags: SectionFlags,
    /// Alignment in bytes (power of two).
    pub align: usize,
    /// Contents (empty for Nobits).
    pub content: Vec<u8>,
}

/// Symbol types (design 6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymType {
    /// Function.
    Func,
    /// Data object.
    Object,
    /// Section symbol.
    Section,
    /// No type.
    Notype,
}

/// Symbol bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymBind {
    /// Local to the object.
    Local,
    /// Global.
    Global,
    /// Weak.
    Weak,
}

/// Symbol visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymVis {
    /// Default.
    Default,
    /// Hidden.
    Hidden,
    /// Protected.
    Protected,
}

/// One symbol table entry (design 6.5).
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Symbol name.
    pub name: String,
    /// Offset within the defining section.
    pub value: usize,
    /// Size in bytes.
    pub size: usize,
    /// Type.
    pub ty: SymType,
    /// Binding.
    pub bind: SymBind,
    /// Visibility.
    pub vis: SymVis,
    /// Defining section name; empty for undefined symbols.
    pub section: String,
}

/// One relocation entry (design 6.4).
#[derive(Debug, Clone)]
pub struct Reloc {
    /// Byte offset within the target section.
    pub offset: usize,
    /// Referenced symbol name.
    pub symbol: String,
    /// Relocation type.
    pub ty: RelocType,
    /// Constant addend.
    pub addend: i64,
}

/// The object under construction.
#[derive(Debug, Clone, Default)]
pub struct Object {
    /// Sections in emission order.
    pub sections: Vec<Section>,
    /// Symbols.
    pub symbols: Vec<Symbol>,
    /// Relocations applying to .text.
    pub relocs: Vec<Reloc>,
}

impl Object {
    /// Adds a section.
    // CEP:WHAT Appends a section.
    // CEP:WHY Emitters build objects incrementally.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn add_section(&mut self, s: Section) {
        self.sections.push(s);
    }

    /// Adds a symbol, rejecting duplicates.
    // CEP:WHAT Registers one symbol.
    // CEP:WHY Invariant 3: symbol completeness without duplicates.
    // CEP:STATUS complete
    // CEP:FAILURE EmitError::DuplicateSymbol on a name clash.
    // CEP:ASSUMES None.
    // CEP:COST O(|symbols|).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn add_symbol(&mut self, s: Symbol) -> Result<(), EmitError> {
        if self.symbols.iter().any(|e| e.name == s.name) {
            return Err(EmitError::DuplicateSymbol { name: s.name });
        }
        self.symbols.push(s);
        Ok(())
    }

    /// Adds a relocation after checking the symbol exists.
    // CEP:WHAT Registers one fixup as a relocation.
    // CEP:WHY Invariant 2: relocation completeness.
    // CEP:STATUS complete
    // CEP:FAILURE EmitError::UndefinedSymbol for unknown symbols.
    // CEP:ASSUMES None.
    // CEP:COST O(|symbols|).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn add_reloc(&mut self, r: Reloc) -> Result<(), EmitError> {
        if !self.symbols.iter().any(|s| s.name == r.symbol) {
            return Err(EmitError::UndefinedSymbol { name: r.symbol });
        }
        self.relocs.push(r);
        Ok(())
    }
}

/// Layout order for emitted sections.
// CEP:WHAT Orders sections per design 6.6.
// CEP:WHY Layout order must satisfy alignment and the documented order.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(log |sections|).
// CEP:EVIDENCE cargo test -p vortex-opt
fn layout_rank(s: &Section) -> usize {
    match s.name.as_str() {
        ".text" => 0,
        ".rodata" => 1,
        ".data" => 2,
        ".bss" => 3,
        ".symtab" => 4,
        ".strtab" => 5,
        n if n.starts_with(".rela") => 6,
        _ => 7,
    }
}

/// The serialized ELF64 object file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Elf {
    /// The full byte image.
    pub bytes: Vec<u8>,
    /// Number of sections emitted, including the null section.
    pub section_count: usize,
}

/// Serializes the object to an ELF64 little-endian image (design 6.7).
// CEP:WHAT Writes the header, laid-out sections with alignment, symbol
//        and string tables, relocations, and the section header table.
// CEP:WHY Format conformance (invariant 5) is the emitter's contract.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; serialization is total.
// CEP:ASSUMES x86-64, SYSV ABI, class 64, little-endian.
// CEP:COST O(total bytes).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn emit_elf(obj: &Object) -> Elf {
    let mut ordered: Vec<&Section> = obj.sections.iter().collect();
    ordered.sort_by_key(|s| layout_rank(s));

    // Symbol string table (used by .symtab entries).
    let mut strtab: Vec<u8> = vec![0u8];
    let mut sym_name_off: Vec<(String, usize)> = Vec::new();
    for sym in &obj.symbols {
        if !sym_name_off.iter().any(|(n, _)| n == &sym.name) {
            sym_name_off.push((sym.name.clone(), strtab.len()));
            strtab.extend_from_slice(sym.name.as_bytes());
            strtab.push(0);
        }
    }

    // Symbol table bytes: null entry first.
    let mut symtab_bytes: Vec<u8> = vec![0u8; 24];
    for sym in &obj.symbols {
        let off = sym_name_off
            .iter()
            .find(|(n, _)| n == &sym.name)
            .map(|(_, o)| *o)
            .unwrap_or(0) as u32;
        symtab_bytes.extend_from_slice(&off.to_le_bytes());
        symtab_bytes.push(sym_info(sym));
        symtab_bytes.push(0u8);
        symtab_bytes.extend_from_slice(&0u16.to_le_bytes());
        symtab_bytes.extend_from_slice(&(sym.value as u64).to_le_bytes());
        symtab_bytes.extend_from_slice(&(sym.size as u64).to_le_bytes());
    }

    // Relocation bytes.
    let mut rela_bytes: Vec<u8> = Vec::new();
    for r in &obj.relocs {
        rela_bytes.extend_from_slice(&(r.offset as u64).to_le_bytes());
        let sym_index = obj
            .symbols
            .iter()
            .position(|s| s.name == r.symbol)
            .map(|i| i as u64 + 1)
            .unwrap_or(0);
        let info = (sym_index << 32)
            | match r.ty {
                RelocType::Absolute64 => 1u64,
                RelocType::Pc32 => 2u64,
                RelocType::Plt32 => 4u64,
                RelocType::JumpSlot => 7u64,
            };
        rela_bytes.extend_from_slice(&info.to_le_bytes());
        rela_bytes.extend_from_slice(&r.addend.to_le_bytes());
    }

    // Meta sections.
    let meta = [
        Section {
            name: ".strtab".to_string(),
            ty: SectionType::Strtab,
            flags: SectionFlags {
                alloc: false,
                write: false,
                exec: false,
            },
            align: 1,
            content: strtab.clone(),
        },
        Section {
            name: ".rela.text".to_string(),
            ty: SectionType::Rela,
            flags: SectionFlags {
                alloc: false,
                write: false,
                exec: false,
            },
            align: 8,
            content: rela_bytes,
        },
        Section {
            name: ".symtab".to_string(),
            ty: SectionType::Symtab,
            flags: SectionFlags {
                alloc: false,
                write: false,
                exec: false,
            },
            align: 8,
            content: symtab_bytes,
        },
    ];

    // Section header string table: built after the final section list is
    // known, including the section's own name.
    let mut all: Vec<&Section> = ordered;
    for m in &meta {
        all.push(m);
    }
    let mut shstrtab: Vec<u8> = vec![0u8];
    let mut sh_name_off: Vec<usize> = Vec::new();
    for s in &all {
        sh_name_off.push(shstrtab.len());
        shstrtab.extend_from_slice(s.name.as_bytes());
        shstrtab.push(0);
    }
    let shstrtab_name_off = shstrtab.len();
    shstrtab.extend_from_slice(b".shstrtab\0");
    let shstrtab_content = shstrtab.clone();

    // Layout with alignment.
    let mut offsets: Vec<u64> = Vec::new();
    let mut offset = 64u64;
    for s in &all {
        let align = s.align.max(1) as u64;
        offset = (offset + align - 1).div_ceil(align) * align;
        offsets.push(offset);
        if s.ty != SectionType::Nobits {
            offset += s.content.len() as u64;
        }
    }
    let shstrtab_offset = offset;
    let shstrtab_size = shstrtab_content.len() as u64;

    // ELF header.
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&1u16.to_le_bytes()); // ET_REL
    bytes.extend_from_slice(&62u16.to_le_bytes()); // EM_X86_64
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes()); // entry
    bytes.extend_from_slice(&0u64.to_le_bytes()); // phoff
    let shoff_at = bytes.len();
    bytes.extend_from_slice(&0u64.to_le_bytes()); // section header table offset, patched after layout
    bytes.extend_from_slice(&0u32.to_le_bytes()); // flags
    bytes.extend_from_slice(&64u16.to_le_bytes()); // ehsize
    bytes.extend_from_slice(&0u16.to_le_bytes()); // phentsize
    bytes.extend_from_slice(&0u16.to_le_bytes()); // phnum
    bytes.extend_from_slice(&64u16.to_le_bytes()); // shentsize
    let shnum = all.len() as u16 + 2; // null + sections + shstrtab
    bytes.extend_from_slice(&shnum.to_le_bytes());
    bytes.extend_from_slice(&(shnum - 1).to_le_bytes()); // shstrndx

    // Section contents.
    for (i, s) in all.iter().enumerate() {
        let align = s.align.max(1) as u64;
        let target = (bytes.len() as u64).div_ceil(align) * align;
        while (bytes.len() as u64) < target {
            bytes.push(0);
        }
        offsets[i] = bytes.len() as u64;
        if s.ty != SectionType::Nobits {
            bytes.extend_from_slice(&s.content);
        }
    }
    bytes.extend_from_slice(&shstrtab_content);

    // Section header table: null, sections, shstrtab.
    let shoff = bytes.len() as u64;
    let mut sh: Vec<u8> = vec![0u8; 64];
    bytes.extend_from_slice(&sh);
    for (i, s) in all.iter().enumerate() {
        let (ty, flags_word): (u32, u64) = match s.ty {
            SectionType::Progbits => (
                1,
                match (s.flags.alloc, s.flags.write, s.flags.exec) {
                    (true, false, true) => 0x6,
                    (true, true, false) => 0x3,
                    (true, false, false) => 0x2,
                    _ => 0,
                },
            ),
            SectionType::Nobits => (8, 0x3),
            SectionType::Symtab => (2, 0),
            SectionType::Strtab => (3, 0),
            SectionType::Rela => (4, 0),
        };
        let (link, info, entsize): (u32, u32, u64) = match s.ty {
            SectionType::Symtab => (
                all.iter()
                    .position(|x| x.name == ".strtab")
                    .map(|p| p as u32 + 1)
                    .unwrap_or(0),
                first_non_local_index(obj),
                24,
            ),
            SectionType::Rela => (
                all.iter()
                    .position(|x| x.name == ".symtab")
                    .map(|p| p as u32 + 1)
                    .unwrap_or(0),
                1,
                24,
            ),
            _ => (0, 0, 0),
        };
        let size = match s.ty {
            SectionType::Nobits => 0u64,
            _ => s.content.len() as u64,
        };
        sh = vec![0u8; 64];
        sh[0..4].copy_from_slice(&(sh_name_off[i] as u32).to_le_bytes());
        sh[4..8].copy_from_slice(&ty.to_le_bytes());
        sh[8..16].copy_from_slice(&flags_word.to_le_bytes());
        sh[16..24].copy_from_slice(&0u64.to_le_bytes()); // addr
        sh[24..32].copy_from_slice(&offsets[i].to_le_bytes());
        let size_bytes = size.to_le_bytes();
        sh[32..40].copy_from_slice(&size_bytes);
        sh[40..44].copy_from_slice(&link.to_le_bytes());
        sh[44..48].copy_from_slice(&info.to_le_bytes());
        sh[48..56].copy_from_slice(&(s.align.max(1) as u64).to_le_bytes());
        sh[56..64].copy_from_slice(&entsize.to_le_bytes());
        bytes.extend_from_slice(&sh);
    }
    // shstrtab header.
    sh = vec![0u8; 64];
    sh[0..4].copy_from_slice(&(shstrtab_name_off as u32).to_le_bytes());
    sh[4..8].copy_from_slice(&3u32.to_le_bytes()); // STRTAB
    sh[24..32].copy_from_slice(&shstrtab_offset.to_le_bytes());
    sh[32..40].copy_from_slice(&shstrtab_size.to_le_bytes());
    bytes.extend_from_slice(&sh);

    // Patch shoff.
    let shoff_bytes = shoff.to_le_bytes();
    bytes[shoff_at..shoff_at + 8].copy_from_slice(&shoff_bytes);

    Elf {
        bytes,
        section_count: shnum as usize,
    }
}

/// The st_info byte of a symbol.
// CEP:WHAT Packs bind and type into st_info.
// CEP:WHY ELF symbol entries carry bind and type in one byte.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt
fn sym_info(sym: &Symbol) -> u8 {
    let bind = match sym.bind {
        SymBind::Local => 0u8,
        SymBind::Global => 1,
        SymBind::Weak => 2,
    };
    let ty = match sym.ty {
        SymType::Func => 2u8,
        SymType::Object => 1,
        SymType::Section => 3,
        SymType::Notype => 0,
    };
    (bind << 4) | ty
}

/// The first non-local symbol index for the symtab sh_info field.
// CEP:WHAT Counts leading local symbols.
// CEP:WHY sh_info of .symtab names the first non-local slot.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(|symbols|).
// CEP:EVIDENCE cargo test -p vortex-opt
fn first_non_local_index(obj: &Object) -> u32 {
    obj.symbols
        .iter()
        .take_while(|s| s.bind == SymBind::Local)
        .count() as u32
}
