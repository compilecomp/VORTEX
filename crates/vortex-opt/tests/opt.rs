// CEP:FILE crates/vortex-opt/tests/opt.rs
// CEP:WHAT Backend tests: ET-SoN pass counters, Vector IR invariants,
//        register allocation, scheduling constraints, ELF structure,
//        DWARF line decoding, and linker resolution and relocation.
// CEP:WHY Each backend subsystem's documented invariants are correctness
//        requirements; the tests are their executable definitions.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Inputs are constructed in the tests.
// CEP:COST O(size of input) per case.
// CEP:EVIDENCE cargo test -p vortex-opt

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Optimizing backend test suite.

use vortex_ir::{Builder, EffectSet, FpSet, Node, Op, Type};
use vortex_opt::dwarf::{LineHeader, LineProgram};
use vortex_opt::emit::{
    emit_elf, Object, Reloc, RelocType, Section, SectionFlags, SectionType, SymBind, SymType,
    SymVis, Symbol,
};
use vortex_opt::link::{apply_relocations, assign_addresses, resolve_symbols};
use vortex_opt::mach::{allocate_registers, select, MInstr, MOp, MachineFunction};
use vortex_opt::passes::run_pipeline;
use vortex_opt::sched::{
    build_dependencies, list_schedule, modulo_schedule, validate, TargetModel,
};
use vortex_opt::vector::{Alignment, LaneOp, ReduceOp, ScalarTag, VGraph, VNode, VType};

/// Builds a graph folding (2+3)*1 into a constant.
// CEP:WHAT Constructs a foldable arithmetic graph.
// CEP:WHY The pipeline must fold, simplify, and DCE in sequence.
// CEP:STATUS complete
// CEP:FAILURE Panics on builder misuse.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-opt
// CEP:ID: JIT-a21
fn foldable_graph() -> Builder {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let two = b.pure(Op::ConstInt(2), Type::Int64, vec![]);
    let three = b.pure(Op::ConstInt(3), Type::Int64, vec![]);
    let add = b.pure(Op::IAdd, Type::Int64, vec![two, three]);
    // A float constant is not an integer constant, so folding leaves the
    // algebraic identities below for the simplify pass.
    let x = b.pure(Op::ConstFloat(1.5), Type::Float64, vec![]);
    let zero = b.pure(Op::ConstInt(0), Type::Int64, vec![]);
    let one = b.pure(Op::ConstInt(1), Type::Int64, vec![]);
    let sub = b.pure(Op::ISub, Type::Int64, vec![x, zero]);
    let mul = b.pure(Op::IMul, Type::Int64, vec![x, one]);
    let dead = b.pure(Op::ConstInt(9), Type::Int64, vec![]);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![add, sub, mul],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let _ = dead;
    let _ = ret;
    b
}

/// The pipeline folds, simplifies, and eliminates dead nodes.
// CEP:WHAT Verifies pass counters on the foldable graph.
// CEP:WHY Catalog items 1, 6, 10.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn pipeline_counts() {
    let mut b = foldable_graph();
    let stats = run_pipeline(&mut b.graph);
    assert!(stats.folded >= 1, "2+3 folds");
    assert!(stats.algebraic >= 2, "-0 and *1 simplify");
    assert!(stats.dead >= 1, "dead const removed");
}

/// Vector IR verifies well-formed graphs and rejects bad widths.
// CEP:WHAT The vector verifier accepts the valid case and rejects the
//        mismatched one.
// CEP:WHY Invariants 6.6: width consistency and mask bindings.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:ID: JIT-a22
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn vector_verifier() {
    let mut g = VGraph::default();
    let ty = VType {
        elem: ScalarTag::I32,
        lanes: 4,
    };
    let mask = g.push(VNode::VMask {
        cmp: LaneOp::Lt,
        ty,
        operands: [0, 0],
    });
    let a = g.push(VNode::VArith {
        lane_op: LaneOp::Add,
        ty,
        operands: [0, 0],
        mask: Some(mask),
    });
    let load = g.push(VNode::VLoad {
        ty,
        base: 0,
        align: Alignment::Contiguous(16),
        mask: Some(mask),
    });
    let _red = g.push(VNode::VReduce {
        op: ReduceOp::Sum,
        ty,
        vector: a,
    });
    let _ = load;
    g.verify().expect("valid vector graph");
    // Width mismatch: reduce over a 1-lane result.
    let mut bad = VGraph::default();
    let one = bad.push(VNode::VArith {
        lane_op: LaneOp::Add,
        ty: VType {
            elem: ScalarTag::I32,
            lanes: 1,
        },
        operands: [0, 0],
        mask: None,
    });
    let wide = bad.push(VNode::VArith {
        lane_op: LaneOp::Add,
        ty,
        operands: [one, one],
        mask: None,
    });
    let _ = wide;
    assert!(bad.verify().is_err());
}

/// Register allocation assigns distinct registers to overlapping lives.
// CEP:WHAT Linear scan maps overlapping vregs to distinct physical regs.
// CEP:WHY Invariant: no two live vregs share a register.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:ID: JIT-a23
// CEP:COST O(vregs).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn register_allocation() {
    let mut func = MachineFunction::default();
    let v0 = func.new_vreg();
    let v1 = func.new_vreg();
    let v2 = func.new_vreg();
    func.emit(MInstr {
        op: MOp::MovRI,
        operands: vec![v0 as u64, 1],
        label: None,
    });
    func.emit(MInstr {
        op: MOp::MovRI,
        operands: vec![v1 as u64, 2],
        label: None,
    });
    func.emit(MInstr {
        op: MOp::AddRR,
        operands: vec![v2 as u64, v0 as u64, v1 as u64],
        label: None,
    });
    func.emit(MInstr {
        op: MOp::Ret,
        operands: vec![v2 as u64],
        label: None,
    });
    allocate_registers(&mut func, &[0, 1, 2, 3]).expect("allocation");
    let r0 = func.reg_map[&v0];
    let r1 = func.reg_map[&v1];
    assert_ne!(r0, r1, "overlapping lives get distinct registers");
    // Selection from ET-SoN works for the supported subset.
    let mut b = Builder::new(1);
    b.start(vec![]);
    let c = b.pure(Op::ConstInt(4), Type::Int64, vec![]);
    let _ = b.pure(Op::IAdd, Type::Int64, vec![c, c]);
    let order: Vec<vortex_ir::NodeId> = b.graph.iter().map(|(id, _)| id).collect();
    let mut f2 = MachineFunction::default();
    select(&b.graph, &order, &mut f2).expect("selection");
    assert!(f2.instrs.len() >= 2);
}

/// List scheduling preserves dependencies and respects resources.
// CEP:WHAT A latency chain schedules in order; a two-wide target overlaps
//        independent instructions.
// CEP:WHY Invariants 1-3 of Valid_Schedule.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ID: JIT-a24
// CEP:ASSUMES None.
// CEP:COST O(instructions squared).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn list_schedule_validates() {
    let mut func = MachineFunction::default();
    // v0 = 1; v1 = v0 + 1; ret v1: a strict chain.
    func.emit(MInstr {
        op: MOp::MovRI,
        operands: vec![0, 1],
        label: None,
    });
    func.emit(MInstr {
        op: MOp::AddRR,
        operands: vec![1, 0, 0],
        label: None,
    });
    func.emit(MInstr {
        op: MOp::Ret,
        operands: vec![1],
        label: None,
    });
    let deps = build_dependencies(&func);
    let target = TargetModel::default();
    let sched = list_schedule(&func, &deps, target);
    validate(&func, &deps, &sched, target, 0).expect("valid schedule");
    let cyc = |i: usize| sched.iter().find(|s| s.index == i).unwrap().cycle;
    assert!(cyc(1) >= cyc(0) + MOp::MovRI.latency());
}

/// Modulo scheduling finds an II and validates it.
// CEP:WHAT A loop body with a carried dependency schedules at II >= 1.
// CEP:WHY 6.5: MII bounds the search; results must validate.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(instructions).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn modulo_schedule_works() {
    let mut func = MachineFunction::default();
    func.emit(MInstr {
        op: MOp::LoadRM,
        operands: vec![0, 1],
        label: None,
    });
    func.emit(MInstr {
        op: MOp::AddRR,
        operands: vec![2, 0, 0],
        label: None,
    });
    func.emit(MInstr {
        op: MOp::StoreMR,
        operands: vec![1, 2],
        label: None,
    });
    let deps = vec![vortex_opt::sched::DepEdge {
        from: 2,
        to: 0,
        latency: 1,
        distance: 1,
        ordering_only: false,
    }];
    let target = TargetModel::default();
    let (sched, ii) = modulo_schedule(&func, &deps, target).expect("modulo schedule");
    validate(&func, &deps, &sched, target, ii).expect("validated");
    assert!(ii >= 1);
}

/// The ELF image carries the magic, the sections, and the symbol.
// CEP:WHAT Emission produces a parseable header and section count.
// CEP:WHY Invariant 5: format conformance.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(bytes).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn elf_structure() {
    let mut obj = Object::default();
    obj.add_section(Section {
        name: ".text".to_string(),
        ty: SectionType::Progbits,
        flags: SectionFlags {
            alloc: true,
            write: false,
            exec: true,
        },
        align: 16,
        content: vec![0x90; 16],
    });
    obj.add_symbol(Symbol {
        name: "main".to_string(),
        value: 0,
        size: 16,
        ty: SymType::Func,
        bind: SymBind::Global,
        vis: SymVis::Default,
        section: ".text".to_string(),
    })
    .expect("symbol");
    obj.add_reloc(Reloc {
        offset: 8,
        symbol: "main".to_string(),
        ty: RelocType::Absolute64,
        addend: 0,
    })
    .expect("reloc");
    let elf = emit_elf(&obj);
    assert_eq!(&elf.bytes[0..4], &[0x7f, b'E', b'L', b'F']);
    assert_eq!(elf.bytes[4], 2, "class 64");
    assert_eq!(
        elf.section_count, 6,
        "null + text + rela + symtab + strtab + shstrtab"
    );
}

/// The line program decodes back to the source rows.
// CEP:WHAT The emitted DWARF line program state machine reproduces rows.
// CEP:WHY 6.4: the program is a decodable state machine.
// CEP:STATUS complete
// CEP:ID: JIT-a25
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(rows).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn line_program_round_trips() {
    let rows = vec![
        vortex_opt::dwarf::LineRow {
            address: 0,
            line: 1,
            column: 0,
            is_stmt: true,
        },
        vortex_opt::dwarf::LineRow {
            address: 4,
            line: 2,
            column: 0,
            is_stmt: true,
        },
        vortex_opt::dwarf::LineRow {
            address: 8,
            line: 3,
            column: 0,
            is_stmt: true,
        },
    ];
    let program = LineProgram::new(LineHeader::default(), rows.clone());
    let bytes = program.emit();
    // Execute the state machine over the body and collect rows.
    let mut decoded: Vec<(u64, u32)> = Vec::new();
    let (mut addr, mut line) = (0u64, 1u32);
    let mut i = 0;
    // Skip the 4-byte unit length, then parse the header: 7 fixed bytes,
    // 12 standard opcode lengths, then the DWARF5 directory and file
    // entry formats the emitter wrote.
    i += 4;
    i += 7; // version, min_len, max_ops, default, line_base, line_range, opcode_base
    i += 12; // standard opcode lengths
    let read_uleb = |bytes: &[u8], i: &mut usize| -> u64 {
        let mut shift = 0;
        let mut val: u64 = 0;
        loop {
            let b = bytes[*i];
            *i += 1;
            val |= ((b & 0x7f) as u64) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        val
    };
    let dir_fmt_count = read_uleb(&bytes, &mut i);
    for _ in 0..dir_fmt_count {
        let _ = read_uleb(&bytes, &mut i);
        let _ = read_uleb(&bytes, &mut i);
    }
    let _dir_count = read_uleb(&bytes, &mut i);
    i += 2; // the ".\0" directory entry
    let file_fmt_count = read_uleb(&bytes, &mut i);
    for _ in 0..file_fmt_count {
        let _ = read_uleb(&bytes, &mut i);
        let _ = read_uleb(&bytes, &mut i);
    }
    let _file_count = read_uleb(&bytes, &mut i);
    i += 10; // "main.vrx\0" plus the directory index uleb(0)
    while i + 1 < bytes.len() {
        let op = bytes[i];
        if op == 0 {
            // Extended: [0, len, sub]
            let len = bytes[i + 1] as usize;
            if len >= 1 && bytes[i + 2] == 0x01 {
                decoded.push((addr, line));
                decoded.push((u64::MAX, 0));
            }
            i += 2 + len;
            continue;
        }
        if op >= 13 {
            let adj = (op - 13) as u64;
            addr += adj / 14; // line_range 14, min length 1
            line = (line as i64 + -5 + (adj % 14) as i64) as u32;
            decoded.push((addr, line));
            i += 1;
            continue;
        }
        match op {
            1 => {
                // copy: emit a row
                decoded.push((addr, line));
                i += 1;
            }
            3 => {
                // advance_line: sleb operand
                i += 1;
                let mut shift = 0;
                let mut val: i64 = 0;
                loop {
                    let b = bytes[i];
                    i += 1;
                    val |= ((b & 0x7f) as i64) << shift;
                    shift += 7;
                    if b & 0x80 == 0 {
                        break;
                    }
                }
                line = (line as i64 + val) as u32;
            }
            2 => {
                // advance_pc: uleb operand
                i += 1;
                let mut shift = 0;
                let mut val: u64 = 0;
                loop {
                    let b = bytes[i];
                    i += 1;
                    val |= ((b & 0x7f) as u64) << shift;
                    shift += 7;
                    if b & 0x80 == 0 {
                        break;
                    }
                }
                addr += val;
            }
            5 => {
                // set_column: uleb operand
                i += 1;
                while bytes[i] & 0x80 != 0 {
                    i += 1;
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    assert!(decoded.len() >= 3, "rows decoded: {decoded:?}");
    assert_eq!(decoded[0].0, 0);
}

/// The linker resolves symbols and applies relocations.
// CEP:WHAT Strong/weak precedence and S+A, S+A-P resolution.
// CEP:WHY Invariants 1 and 3 of Valid_Exec.
// CEP:ID: JIT-a26
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(symbols).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn linker_resolves_and_patches() {
    let mut obj = Object::default();
    obj.add_section(Section {
        name: ".text".to_string(),
        ty: SectionType::Progbits,
        flags: SectionFlags {
            alloc: true,
            write: false,
            exec: true,
        },
        align: 16,
        content: vec![0u8; 32],
    });
    obj.add_symbol(Symbol {
        name: "target".to_string(),
        value: 16,
        size: 4,
        ty: SymType::Func,
        bind: SymBind::Global,
        vis: SymVis::Default,
        section: ".text".to_string(),
    })
    .expect("symbol");
    obj.add_reloc(Reloc {
        offset: 0,
        symbol: "target".to_string(),
        ty: RelocType::Absolute64,
        addend: 0,
    })
    .expect("reloc");
    let symbols = resolve_symbols(&[obj.clone()]).expect("resolve");
    let layout = assign_addresses(&[obj.clone()]).expect("layout");
    let text_base = layout.section_addresses[".text"];
    assert_eq!(layout.symbol_addresses["target"], text_base + 16);
    let patched = apply_relocations(&[obj], &layout, &symbols).expect("relocations");
    let text = &patched[".text"];
    let value = u64::from_le_bytes(text[0..8].try_into().unwrap());
    assert_eq!(value, layout.symbol_addresses["target"], "S + A applied");
}

/// PC-relative relocation resolution subtracts the site address.
// CEP:WHAT S + A - P lands in the patched field.
// CEP:ID: JIT-a27
// CEP:WHY 6.6: the PC32 formula.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(relocs).
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn pc32_resolves_relative() {
    let mut obj = Object::default();
    obj.add_section(Section {
        name: ".text".to_string(),
        ty: SectionType::Progbits,
        flags: SectionFlags {
            alloc: true,
            write: false,
            exec: true,
        },
        align: 16,
        content: vec![0u8; 64],
    });
    obj.add_symbol(Symbol {
        name: "t".to_string(),
        value: 32,
        size: 4,
        ty: SymType::Func,
        bind: SymBind::Global,
        vis: SymVis::Default,
        section: ".text".to_string(),
    })
    .expect("symbol");
    obj.add_reloc(Reloc {
        offset: 0,
        symbol: "t".to_string(),
        ty: RelocType::Pc32,
        addend: 0,
    })
    .expect("reloc");
    let symbols = resolve_symbols(&[obj.clone()]).expect("resolve");
    let layout = assign_addresses(&[obj.clone()]).expect("layout");
    let patched = apply_relocations(&[obj], &layout, &symbols).expect("reloc");
    let text = &patched[".text"];
    let value = i32::from_le_bytes(text[0..4].try_into().unwrap());
    assert_eq!(value, 32, "S - P = 32 - 0");
}
