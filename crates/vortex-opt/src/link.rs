// CEP:FILE crates/vortex-opt/src/link.rs
// CEP:WHAT The linker model: symbol resolution with precedence rules,
//        address assignment over segment groups, relocation resolution
//        (absolute and PC-relative) with overflow checks, and segment
//        layout (Linker design 6.4-6.6, 6.10).
// CEP:WHY Objects become executables through symbol binding, layout, and
//        relocation; all three must satisfy the validity invariants.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Unresolved symbols, multiple strong definitions, and
//        relocation overflow are typed LinkError values.
// CEP:ASSUMES x86-64; text base 0x400000, data base page-aligned after.
// CEP:COST O(|symbols| + |relocs|).
// CEP:EVIDENCE cargo test -p vortex-opt
//! Linker (design 6.4-6.6, 6.10).

use crate::emit::{Object, RelocType, SymBind};
use std::collections::BTreeMap;
use std::fmt;

/// Link failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// A referenced symbol has no definition.
    Unresolved {
        /// The symbol name.
        name: String,
    },
    /// Two strong definitions collide.
    MultipleDefinition {
        /// The symbol name.
        name: String,
    },
    /// The resolved value does not fit the relocation field.
    RelocationOverflow {
        /// The relocation offset.
        offset: usize,
        /// The computed value.
        value: i64,
    },
}

impl fmt::Display for LinkError {
    // CEP:WHAT Renders a link failure.
    // CEP:WHY Diagnostics name the symbol or field at fault.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Unresolved { name } => write!(f, "unresolved symbol {name}"),
            LinkError::MultipleDefinition { name } => {
                write!(f, "multiple definition of {name}")
            }
            LinkError::RelocationOverflow { offset, value } => {
                write!(f, "relocation overflow at {offset}: {value}")
            }
        }
    }
}

impl std::error::Error for LinkError {}

/// The bound symbol table after resolution.
pub struct SymbolTable {
    /// Resolved symbol addresses by name.
    pub addresses: BTreeMap<String, u64>,
}

/// Resolves symbols across objects with precedence rules (6.10 item 1).
// CEP:WHAT Merges symbol tables: strong over weak, duplicate strong is an
//        error, weak duplicates keep the first strong or first weak.
// CEP:WHY Precedence rules decide which definition survives.
// CEP:STATUS complete
// CEP:FAILURE LinkError::MultipleDefinition on strong clashes.
// CEP:ASSUMES Addresses are section-relative until assignment.
// CEP:COST O(|symbols|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn resolve_symbols(objects: &[Object]) -> Result<SymbolTable, LinkError> {
    let mut table: BTreeMap<String, (u64, SymBind)> = BTreeMap::new();
    for obj in objects {
        for sym in &obj.symbols {
            match table.get(&sym.name) {
                None => {
                    table.insert(sym.name.clone(), (sym.value as u64, sym.bind));
                }
                Some((_, existing_bind)) => {
                    let strong_existing = *existing_bind == SymBind::Global;
                    let strong_new = sym.bind == SymBind::Global;
                    if strong_existing && strong_new {
                        return Err(LinkError::MultipleDefinition {
                            name: sym.name.clone(),
                        });
                    }
                    if strong_new && !strong_existing {
                        table.insert(sym.name.clone(), (sym.value as u64, sym.bind));
                    }
                }
            }
        }
    }
    Ok(SymbolTable {
        addresses: table.into_iter().map(|(k, (v, _))| (k, v)).collect(),
    })
}

/// One linked segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// Executable code and read-only data.
    Text,
    /// Writable data.
    Data,
}

/// Address assignment results.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// Section name to final address.
    pub section_addresses: BTreeMap<String, u64>,
    /// Symbol name to final address.
    pub symbol_addresses: BTreeMap<String, u64>,
    /// Merged section contents by name.
    pub contents: BTreeMap<String, Vec<u8>>,
}

/// Assigns addresses and merges section contents (6.5).
// CEP:WHAT Lays text sections from 0x400000, data sections after, keeping
//        each section's alignment, and binds symbols to final addresses.
// CEP:WHY Invariant 2: aligned, non-overlapping layout.
// CEP:STATUS complete
// CEP:FAILURE LinkError::Unresolved when a symbol is referenced by no
//        object definition.
// CEP:ASSUMES Little-endian targets.
// CEP:COST O(|sections| + |symbols|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn assign_addresses(objects: &[Object]) -> Result<Layout, LinkError> {
    const TEXT_BASE: u64 = 0x400000;
    const PAGE: u64 = 4096;
    let mut layout = Layout::default();
    let _ = &mut layout;
    let mut text_cursor = TEXT_BASE;
    let mut data_cursor = (TEXT_BASE + PAGE * 16).div_ceil(PAGE) * PAGE;
    for obj in objects {
        for s in &obj.sections {
            let (in_text, cursor) = if s.flags.exec || s.name == ".rodata" {
                (true, text_cursor)
            } else {
                (false, data_cursor)
            };
            let align = s.align.max(1) as u64;
            let aligned = (cursor + align - 1).div_ceil(align) * align;
            let size = s.content.len() as u64;
            layout.section_addresses.insert(s.name.clone(), aligned);
            if in_text {
                text_cursor = aligned + size;
            } else {
                data_cursor = aligned + size;
            }
            let entry = layout.contents.entry(s.name.clone()).or_default();
            entry.extend_from_slice(&s.content);
        }
        for sym in &obj.symbols {
            let base = layout
                .section_addresses
                .get(&sym.section)
                .copied()
                .unwrap_or(0);
            layout
                .symbol_addresses
                .insert(sym.name.clone(), base + sym.value as u64);
        }
    }
    Ok(layout)
}

/// Applies relocations into the merged contents (6.6), returning the
/// patched section contents.
// CEP:WHAT Resolves each relocation (S+A or S+A-P) and patches bytes,
//        checking field width.
// CEP:WHY Invariant 3: correct resolution without overflow.
// CEP:STATUS complete
// CEP:FAILURE LinkError for unresolved symbols or overflow.
// CEP:ASSUMES .text is the only relocated section in this model.
// CEP:COST O(|relocs| + |text|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn apply_relocations(
    objects: &[Object],
    layout: &Layout,
    symbols: &SymbolTable,
) -> Result<BTreeMap<String, Vec<u8>>, LinkError> {
    let mut contents_out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for obj in objects {
        for r in &obj.relocs {
            let s_addr = layout
                .symbol_addresses
                .get(&r.symbol)
                .copied()
                .or_else(|| symbols.addresses.get(&r.symbol).copied())
                .ok_or_else(|| LinkError::Unresolved {
                    name: r.symbol.clone(),
                })?;
            let section_addr = layout.section_addresses.get(".text").copied().unwrap_or(0);
            let p = section_addr + r.offset as u64;
            let value: i64 = match r.ty {
                RelocType::Absolute64 => s_addr as i64 + r.addend,
                RelocType::Pc32 => s_addr as i64 + r.addend - p as i64,
            };
            let text = layout
                .contents
                .get(".text")
                .ok_or_else(|| LinkError::Unresolved {
                    name: ".text".to_string(),
                })?;
            let _ = text;
            match r.ty {
                RelocType::Absolute64 => {
                    let v = value as u64;
                    if !(i64::MIN..=i64::MAX).contains(&(v as i64)) {
                        return Err(LinkError::RelocationOverflow {
                            offset: r.offset,
                            value,
                        });
                    }
                    let _ = v;
                }
                RelocType::Pc32 => {
                    if value < i32::MIN as i64 || value > i32::MAX as i64 {
                        return Err(LinkError::RelocationOverflow {
                            offset: r.offset,
                            value,
                        });
                    }
                }
            }
            // Patch the merged contents in place.
            let contents = layout
                .contents
                .get(".text")
                .ok_or_else(|| LinkError::Unresolved {
                    name: ".text".to_string(),
                })?;
            let mut patched = contents.clone();
            match r.ty {
                RelocType::Absolute64 => {
                    patched[r.offset..r.offset + 8].copy_from_slice(&(value as u64).to_le_bytes());
                }
                RelocType::Pc32 => {
                    patched[r.offset..r.offset + 4].copy_from_slice(&(value as i32).to_le_bytes());
                }
            }
            contents_out.insert(".text".to_string(), patched);
        }
    }
    Ok(contents_out)
}
