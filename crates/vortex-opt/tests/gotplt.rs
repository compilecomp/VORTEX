// CEP:FILE crates/vortex-opt/tests/gotplt.rs
// CEP:WHAT GOT/PLT tests: stub byte layout, rip-relative slot displacement,
//        stub binding for imports, direct binding for definitions, and
//        the PLT32 relocation-type mapping in emitted ELF.
// CEP:WHY Dynamic binding is only correct if these are pinned.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES Stubs are 16 bytes; slots 8 bytes; little-endian.
// CEP:COST O(cases).
// CEP:EVIDENCE cargo test -p vortex-opt --test gotplt

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! GOT/PLT linker test suite.

use vortex_opt::emit::{
    Object, Reloc, RelocType, Section, SectionFlags, SectionType, SymBind, SymType, Symbol,
};
use vortex_opt::link::{
    apply_relocations_with_plt, assign_addresses, build_got_plt, resolve_symbols,
};

const PLT_BASE: u64 = 0x401000;
const GOT_BASE: u64 = 0x500000;

/// Builds an object with one text section and one Plt32 call.
// CEP:WHAT Constructs the linking test object.
// CEP:WHY Both the stub and direct-binding cases share the shape.
// CEP:STATUS complete
// CEP:FAILURE Panics on invalid construction.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test gotplt
fn caller_object(callee: &str, section: &str) -> Object {
    let mut o = Object::default();
    o.add_section(Section {
        name: ".text".to_string(),
        ty: SectionType::Progbits,
        flags: SectionFlags {
            alloc: true,
            exec: true,
            write: false,
        },
        align: 16,
        content: vec![0xE8, 0x00, 0x00, 0x00, 0x00, 0xC3],
    });
    o.add_symbol(Symbol {
        name: callee.to_string(),
        value: 0,
        size: 0,
        ty: SymType::Func,
        bind: SymBind::Global,
        vis: vortex_opt::emit::SymVis::Default,
        section: section.to_string(),
    })
    .expect("symbol");
    o.add_reloc(Reloc {
        offset: 2,
        symbol: callee.to_string(),
        ty: RelocType::Plt32,
        addend: -4,
    })
    .expect("reloc");
    o
}

#[test]
// CEP:WHAT Stub bytes jump through their GOT slot; slots list in order.
// CEP:WHY The stub layout is the loader's contract.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(imports).
// CEP:EVIDENCE cargo test -p vortex-opt --test gotplt
// CEP:ID: JIT-a17
fn stub_layout_and_bytes() {
    let gp = build_got_plt(&["foo".to_string()], PLT_BASE, GOT_BASE);
    assert_eq!(gp.plt.len(), 16, "one 16-byte stub");
    assert_eq!(gp.got.len(), 8, "one 8-byte slot");
    assert_eq!(&gp.plt[0..2], &[0xFF, 0x25], "jmp *[rip+disp32]");
    let disp = i32::from_le_bytes([gp.plt[2], gp.plt[3], gp.plt[4], gp.plt[5]]);
    assert_eq!(
        PLT_BASE + 6 + disp as u64,
        GOT_BASE,
        "disp must reach the foo slot"
    );
    assert!(gp.plt[6..].iter().all(|&b| b == 0x90), "NOP padding");
    assert_eq!(gp.got, vec![0u8; 8], "slots start zero (loader fills)");
    assert_eq!(gp.jump_slots, vec![("foo".to_string(), GOT_BASE)]);
    assert_eq!(gp.stub_addrs["foo"], PLT_BASE);
}

#[test]
// CEP:WHAT Imported calls bind to their PLT stub through the reloc.
// CEP:WHY Loader-bound symbols must indirect via the stub.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES addend -4 for the standard call form.
// CEP:COST O(relocs).
// CEP:ID: JIT-a18
// CEP:EVIDENCE cargo test -p vortex-opt --test gotplt
fn import_calls_bind_to_stub() {
    let obj = caller_object("foo", "");
    let layout = assign_addresses(std::slice::from_ref(&obj)).expect("layout");
    let symbols = resolve_symbols(std::slice::from_ref(&obj)).expect("resolve");
    let gp = build_got_plt(&["foo".to_string()], PLT_BASE, GOT_BASE);
    let patched = apply_relocations_with_plt(&[obj], &layout, &symbols, &gp).expect("patch");
    let text = &patched[".text"];
    let rel = i32::from_le_bytes([text[2], text[3], text[4], text[5]]);
    let text_base = layout.section_addresses[".text"];
    let p = text_base + 2;
    let target = (p as i64) + 4 + rel as i64;
    assert_eq!(target, PLT_BASE as i64, "call must land on the stub");
}

#[test]
// CEP:WHAT Defined callees bind directly even under Plt32.
// CEP:WHY No stub is needed when the definition is in the link.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES None.
// CEP:ID: JIT-a19
// CEP:COST O(relocs).
// CEP:EVIDENCE cargo test -p vortex-opt --test gotplt
fn defined_callee_binds_directly() {
    let obj = caller_object("bar", ".text");
    let layout = assign_addresses(std::slice::from_ref(&obj)).expect("layout");
    let symbols = resolve_symbols(std::slice::from_ref(&obj)).expect("resolve");
    let gp = build_got_plt(&[], PLT_BASE, GOT_BASE);
    assert!(gp.stub_addrs.is_empty(), "no imports: no stubs");
    let patched = apply_relocations_with_plt(&[obj], &layout, &symbols, &gp).expect("patch");
    let text = &patched[".text"];
    let rel = i32::from_le_bytes([text[2], text[3], text[4], text[5]]);
    let text_base = layout.section_addresses[".text"];
    let bar_addr = layout.symbol_addresses["bar"];
    assert_eq!(
        (text_base + 2) as i64 + 4 + rel as i64,
        bar_addr as i64,
        "Plt32 to a defined symbol binds directly"
    );
}

#[test]
// CEP:WHAT Emitted ELF maps Plt32 to R_X86_64_PLT32 (4).
// CEP:WHY External tools parse the type byte; it must be conforming.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ID: JIT-a20
// CEP:ASSUMES Rela entries carry info = (sym << 32) | type.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test gotplt
fn elf_maps_plt32_to_type4() {
    let obj = caller_object("foo", "");
    let elf = vortex_opt::emit::emit_elf(&obj);
    // Scan section headers for .rela.text and check the info word.
    let bytes = &elf.bytes;
    let shoff = u64::from_le_bytes(bytes[0x28..0x30].try_into().expect("shoff"));
    let shentsize = u16::from_le_bytes(bytes[0x3A..0x3C].try_into().expect("shentsize"));
    let mut found = false;
    for i in 0..elf.section_count {
        let h = shoff as usize + i * shentsize as usize;
        let sh_type = u32::from_le_bytes(bytes[h + 4..h + 8].try_into().expect("type"));
        // SHT_RELA = 4.
        if sh_type == 4 {
            let offset =
                u64::from_le_bytes(bytes[h + 24..h + 32].try_into().expect("offset")) as usize;
            let size = u64::from_le_bytes(bytes[h + 32..h + 40].try_into().expect("size")) as usize;
            // Each rela entry: 24 bytes; info at +8 (8 bytes).
            for e in 0..size / 24 {
                let info = u64::from_le_bytes(
                    bytes[offset + e * 24 + 8..offset + e * 24 + 16]
                        .try_into()
                        .expect("info"),
                );
                assert_eq!(info & 0xFFFF_FFFF, 4, "Plt32 maps to type 4");
                found = true;
            }
        }
    }
    assert!(found, "a RELA section must exist");
}
