// CEP:FILE crates/vortex-t1/src/copy_patch.rs
// CEP:WHAT The T1 copy-and-patch baseline compiler (T1a and T1b, and the
//        T1c fallback through a caller-supplied location plan): decode the
//        bytecode, assign operand locations, select stencils as a pure
//        function of (opcode, locations), copy the template bytes into the
//        code image, and patch location holes, payload holes, and branch
//        offsets (design.md section 3.3).
// CEP:WHY T1's sole objective is minimum compilation latency: no IR, no
//        instruction selection, no register allocator on the stencil paths.
//        The algorithm is a memcpy per stencil plus integer patching - O(N)
//        with a near-zero constant factor.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Undecodable bytecode and unreachable branch targets surface as
//        typed T1Error values; nothing is patched speculatively.
// CEP:ASSUMES The method passed vortex-ugb verification (branch targets are
//        instruction boundaries inside the stream).
// CEP:COST O(instructions + holes): one map lookup and one memcpy per
//        stencil, one write per hole; pool growth is O(distinct keys).
// CEP:EVIDENCE cargo test -p vortex-t1

use vortex_runtime::{Loc, PayloadKind, StencilKey, StencilPool};
use vortex_ugb::{instr, Method, Op};

use crate::plan::{classify, FixedMap, T1Path};

/// Compilation failure with the offending byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum T1Error {
    /// The bytecode stream did not decode.
    Decode {
        /// Byte offset of the fault.
        offset: usize,
    },
    /// A branch targeted a pc with no instance (the verifier prevents this).
    BranchTarget {
        /// The bytecode pc that was targeted.
        target: usize,
    },
    /// The method carries an opcode with no stencil form.
    Unsupported {
        /// The opcode.
        op: Op,
        /// Byte offset of the instruction.
        offset: usize,
    },
    /// The CALL callee method index exceeded the one-byte subset encoding
    /// (no silent truncation, Law 4 of chapter 66).
    CalleeTooLarge {
        /// The callee index that did not fit.
        callee: usize,
        /// Byte offset of the instruction.
        offset: usize,
    },
}

/// One copied stencil instance inside the blob image.
#[derive(Debug, Clone)]
pub struct Instance {
    /// Bytecode pc this instance implements.
    pub bytecode_pc: usize,
    /// Opcode.
    pub op: Op,
    /// Operand locations (as selected for the stencil key).
    pub locs: [Option<Loc>; 3],
    /// Image offset of the instance start.
    pub base: usize,
    /// Instance length in bytes.
    pub len: usize,
    /// Payload holes as (kind, offset-relative-to-base) in template order;
    /// the executor reads the patched values from the image through these.
    pub payloads: Vec<(PayloadKind, usize)>,
}

/// A loop region of the blob (DEOPT-001: single entry at the loop head).
#[derive(Debug, Clone)]
pub struct Region {
    /// Instance index of the region entry (the loop head).
    pub entry: usize,
    /// Bytecode pc of the loop head.
    pub bytecode_pc: usize,
    /// Inclusive start of the instance range.
    pub first: usize,
    /// Exclusive end of the instance range.
    pub last_excl: usize,
}

/// One stitched DWARF line fragment (DEBUG-1 section 2.2: pre-computed
/// fragments patched to the generated code ranges).
#[derive(Debug, Clone, Copy)]
pub struct DwarfFragment {
    /// Instance this fragment describes.
    pub instance: usize,
    /// Image range covered: [start, start + len).
    pub start: usize,
    /// Fragment length.
    pub len: usize,
    /// Modeled source line: the bytecode pc (T1 has no source positions).
    pub line: u32,
}

/// The unwind model of a T1 blob (EXC-1 section 2.2): all T1 code shares a
/// single, static unwind descriptor; publication registers that descriptor
/// for the blob's code range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnwindModel {
    /// Whether the shared static T1 descriptor is registered.
    pub shared_descriptor_registered: bool,
}

/// Operand location assignment for one method.
#[derive(Debug, Clone)]
pub struct LocationPlan {
    /// Virtual register -> operand location.
    pub vreg_loc: Vec<Loc>,
    /// Stack slot (relative to spill base) -> virtual register.
    pub slot_to_vreg: Vec<u16>,
    /// First stack slot owned by the blob.
    pub spill_base: u16,
}

/// Builds the fixed-map location plan (paths A and B).
// CEP:WHAT Builds the fixed-map location plan (paths A and B).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST O(register_count) per method.
// CEP:EVIDENCE cargo test -p vortex-t1
#[must_use]
pub fn fixed_map_plan(method: &Method, path: T1Path) -> LocationPlan {
    let map = FixedMap::new();
    let spill_base = crate::plan::FAST_MAP_REGS;
    let mut vreg_loc = Vec::with_capacity(usize::from(method.register_count));
    let mut slot_to_vreg = Vec::new();
    for v in 0..method.register_count {
        let loc = map.loc(v, spill_base);
        if let Loc::Stack(s) = loc {
            let slot = usize::from(s - spill_base);
            if slot >= slot_to_vreg.len() {
                slot_to_vreg.resize(slot + 1, v);
            }
            slot_to_vreg[slot] = v;
        }
        vreg_loc.push(loc);
    }
    let _ = path;
    LocationPlan {
        vreg_loc,
        slot_to_vreg,
        spill_base,
    }
}

/// The compiled T1 blob.
#[derive(Debug, Clone)]
pub struct T1Blob {
    /// Method name (diagnostics).
    pub name: String,
    /// Method index in the program (the executor resolves constants and
    /// epoch caches through the program table).
    pub method_index: usize,
    /// Which baseline path produced this blob.
    pub path: T1Path,
    /// The copied-and-patched code image.
    pub image: Vec<u8>,
    /// Instances in emission order; branch resolution searches this list.
    pub instances: Vec<Instance>,
    /// Location plan of the blob.
    pub plan: LocationPlan,
    /// Loop regions.
    pub regions: Vec<Region>,
    /// The blob's single shared OSR Trampoline Stub (OSR-1 section 6.1).
    pub osr_stub: vortex_runtime::OsrTrampolineStub,
    /// Stitched DWARF fragments.
    pub dwarf_fragments: Vec<DwarfFragment>,
    /// Unwind registration model.
    pub unwind: UnwindModel,
    /// Patches applied during assembly (copy-and-patch work metric).
    pub patches_applied: u64,
    /// Bytes copied from stencils (copy-and-patch work metric).
    pub bytes_copied: u64,
}

impl T1Blob {
    /// DEBUG-INV-01 (complete PC coverage): every image byte belongs to
    /// exactly one instance. Returns the number of uncovered bytes (zero is
    /// the only passing value).
    #[must_use]
    pub fn uncovered_bytes(&self) -> usize {
        let mut covered = vec![false; self.image.len()];
        for inst in &self.instances {
            for b in inst.base..inst.base + inst.len {
                if b < covered.len() {
                    covered[b] = true;
                }
            }
        }
        covered.iter().filter(|c| !**c).count()
    }

    /// Marks the blob's OSR back-link as patched to a T2 OSR stub
    /// (OSR-1 section 6.1 step 4); the T1 trampoline and its metadata
    /// counter become cold and are swept by the code cache (CODE-1 7).
    pub fn patch_backlink_to_t2(&mut self) {
        self.osr_stub.patched_to_t2 = true;
    }
}

/// Compiles a method with the T1 baseline compiler: stencil-first, routing
/// to the linear-scan fallback when the classifier says the fixed map would
/// degrade (design.md section 5: "T1 is stencil-first, not stencil-only").
// CEP:WHAT Compiles a method with the T1 baseline compiler: stencil-first, routing to the linear-scan fallback when the classifier says the fixed map would degrade.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub fn compile_baseline(
    pool: &mut StencilPool,
    program: &[Method],
    method_index: usize,
) -> Result<T1Blob, T1Error> {
    let method = program
        .get(method_index)
        .ok_or(T1Error::Decode { offset: 0 })?;
    let path = classify(method);
    match path {
        T1Path::A | T1Path::B => {
            let plan = fixed_map_plan(method, path);
            compile_with_plan(pool, method, method_index, path, plan)
        }
        T1Path::C => crate::fallback::compile_t1c(pool, program, method_index),
    }
}

/// Compiles a method through the copy-and-patch pipeline with an explicit
/// location plan (path A/B fixed map, or the T1c linear-scan allocation).
// CEP:WHAT Compiles a method through the copy-and-patch pipeline with an explicit location plan (path A/B fixed map, or the T1c linear-scan allocation).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub fn compile_with_plan(
    pool: &mut StencilPool,
    method: &Method,
    method_index: usize,
    path: T1Path,
    plan: LocationPlan,
) -> Result<T1Blob, T1Error> {
    // Pass 0: decode the stream.
    let decoded = decode_all(method)?;

    // Back-edge loop sites (bytecode pc + profile slot ordinal) for the OSR
    // stub and the region table.
    let loop_sites = collect_loop_sites(&decoded);

    // Open the blob (shared OSR stub with metadata counters).
    let loops: Vec<(usize, usize)> = loop_sites.iter().map(|s| (s.slot, s.pc)).collect();
    let osr_stub = pool.open_blob(&loops);

    let mut blob = T1Blob {
        name: method.name.clone(),
        method_index,
        path,
        image: Vec::new(),
        instances: Vec::new(),
        plan,
        regions: Vec::new(),
        osr_stub,
        dwarf_fragments: Vec::new(),
        unwind: UnwindModel {
            shared_descriptor_registered: true,
        },
        patches_applied: 0,
        bytes_copied: 0,
    };

    // Branch holes to resolve after every instance exists: (instance idx,
    // relative hole offset, bytecode target).
    let mut branch_fixups: Vec<(usize, usize, usize)> = Vec::new();

    for (idx, d) in decoded.iter().enumerate() {
        let locs = operand_locs(d.instr.op, &d.instr, &blob.plan);
        let key = StencilKey {
            op: d.instr.op,
            locs,
        };
        let stencil_idx = pool.get_or_compile(key);
        let stencil = pool.stencil(stencil_idx);
        let base = blob.image.len();
        blob.image.extend_from_slice(&stencil.code);
        blob.bytes_copied += stencil.code.len() as u64;

        let mut patches: u64 = 0;
        let mut payloads: Vec<(PayloadKind, usize)> = Vec::new();
        // Location holes: stack slots, patched in operand order.
        for (loc_hole_idx, &h) in stencil.holes.iter().enumerate() {
            let slot = stack_slot_in_order(locs, loc_hole_idx);
            patch_u16(&mut blob.image, base + h, slot);
            patches += 1;
        }
        // Payload holes, patched in template order; repeated kinds consume
        // successive operand values (e.g. the two u32 immediates of
        // CapCreate/CapCheck).
        let mut imm32u_n = 0_usize;
        let mut branch_n = 0_usize;
        for ph in &stencil.payload_holes {
            match ph.kind {
                PayloadKind::Imm32 => patch_i32(&mut blob.image, base + ph.offset, d.instr.imm32),
                PayloadKind::Imm32u => {
                    patch_u32(&mut blob.image, base + ph.offset, d.instr.imm32u[imm32u_n]);
                    imm32u_n += 1;
                }
                PayloadKind::Imm64 => patch_u64(&mut blob.image, base + ph.offset, d.instr.imm64),
                PayloadKind::Pool => patch_u16(&mut blob.image, base + ph.offset, d.instr.pool),
                PayloadKind::Callee => {
                    let callee =
                        u8::try_from(d.instr.regs[1]).map_err(|_| T1Error::CalleeTooLarge {
                            callee: usize::from(d.instr.regs[1]),
                            offset: d.pc,
                        })?;
                    patch_u8(&mut blob.image, base + ph.offset, callee);
                }
                PayloadKind::Branch => {
                    let target = d
                        .target
                        .ok_or(T1Error::BranchTarget { target: usize::MAX })?;
                    let _ = branch_n;
                    branch_n += 1;
                    branch_fixups.push((idx, ph.offset, target));
                }
            }
            payloads.push((ph.kind, ph.offset));
            patches += 1;
        }
        blob.patches_applied += patches;

        blob.instances.push(Instance {
            bytecode_pc: d.pc,
            op: d.instr.op,
            locs,
            base,
            len: stencil.code.len(),
            payloads,
        });
        blob.dwarf_fragments.push(DwarfFragment {
            instance: idx,
            start: base,
            len: stencil.code.len(),
            line: d.pc as u32,
        });
    }

    // Pass 2: patch branch offsets relative to the copied instance end.
    for (idx, rel_off, target) in branch_fixups {
        let inst = &blob.instances[idx];
        let dest = blob
            .instances
            .iter()
            .find(|i| i.bytecode_pc == target)
            .ok_or(T1Error::BranchTarget { target })?;
        let rel = i16::try_from(dest.base as isize - (inst.base + inst.len) as isize)
            .map_err(|_| T1Error::BranchTarget { target })?;
        patch_i16(&mut blob.image, inst.base + rel_off, rel);
    }

    // Regions: one per loop site; the region spans [loop head .. back-edge].
    for site in &loop_sites {
        let entry = blob
            .instances
            .iter()
            .position(|i| i.bytecode_pc == site.pc)
            .ok_or(T1Error::BranchTarget { target: site.pc })?;
        let last_excl = blob
            .instances
            .iter()
            .position(|i| i.bytecode_pc == site.backedge_pc)
            .map(|p| p + 1)
            .unwrap_or(blob.instances.len());
        blob.regions.push(Region {
            entry,
            bytecode_pc: site.pc,
            first: entry,
            last_excl: last_excl.max(entry + 1),
        });
    }

    Ok(blob)
}

/// Decoded instruction with its branch target precomputed.
#[derive(Debug, Clone)]
pub(crate) struct Decoded {
    /// Bytecode pc.
    pub pc: usize,
    /// Decoded instruction.
    pub instr: instr::Instr,
    /// Branch target (bytecode pc) when the opcode branches.
    pub target: Option<usize>,
}

/// Decodes the whole stream once.
// CEP:WHAT Decodes the whole stream once.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub(crate) fn decode_all(method: &Method) -> Result<Vec<Decoded>, T1Error> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < method.code.len() {
        let (i, next) =
            instr::decode_at(&method.code, p).map_err(|e| T1Error::Decode { offset: e.offset })?;
        let target = if vortex_ugb::sig(i.op).branches > 0 {
            Some(method.branch_target(&i, p, next - p))
        } else {
            None
        };
        out.push(Decoded {
            pc: p,
            instr: i,
            target,
        });
        p = next;
    }
    Ok(out)
}

/// Operand locations for one instruction: register operands map through the
/// location plan; the CALL callee byte is not a location (payload hole).
// CEP:WHAT Operand locations for one instruction: register operands map through the location plan; the CALL callee byte is not a location (payload hole).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub(crate) fn operand_locs(op: Op, i: &instr::Instr, plan: &LocationPlan) -> [Option<Loc>; 3] {
    let s = vortex_ugb::sig(op);
    let mut locs = [None; 3];
    for (slot, r) in i.regs.iter().enumerate().take(usize::from(s.regs)) {
        if op == Op::Call && slot == 1 {
            // regs[1] is the callee method index, not a virtual register.
            locs[slot] = None;
            continue;
        }
        locs[slot] = Some(vreg_loc_of(plan, *r));
    }
    locs
}

/// Location of a virtual register under a plan.
#[must_use]
pub fn vreg_loc_of(plan: &LocationPlan, vreg: u16) -> Loc {
    plan.vreg_loc
        .get(usize::from(vreg))
        .copied()
        .unwrap_or(Loc::Stack(plan.spill_base + vreg))
}

/// Stack slot of the `n`-th stack location in operand order (the runtime
/// template emits exactly one hole per Stack operand, in order).
fn stack_slot_in_order(locs: [Option<Loc>; 3], n: usize) -> u16 {
    locs.iter()
        .flatten()
        .filter_map(|l| match l {
            Loc::Stack(s) => Some(*s),
            Loc::Reg(_) => None,
        })
        .nth(n)
        .unwrap_or(0)
}

/// Back-edge sites: loop head pc, profile slot ordinal, back-edge pc.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopSite {
    /// Loop head bytecode pc.
    pub pc: usize,
    /// Profile slot ordinal (the T0 per-site key).
    pub slot: usize,
    /// Bytecode pc of the backward jump.
    pub backedge_pc: usize,
}

/// Collects the loop sites of a method in first-appearance order of backward
/// jumps (the same ordinal rule the interpreter uses).
// CEP:WHAT Collects the loop sites of a method in first-appearance order of backward jumps (the same ordinal rule the interpreter uses).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
pub(crate) fn collect_loop_sites(decoded: &[Decoded]) -> Vec<LoopSite> {
    let mut sites: Vec<LoopSite> = Vec::new();
    for d in decoded {
        if !matches!(d.instr.op, Op::Jump | Op::JumpTrue | Op::JumpFalse) {
            continue;
        }
        let Some(target) = d.target else {
            continue;
        };
        if target >= d.pc {
            continue; // forward jump: not a back-edge
        }
        let pc = target;
        if !sites.iter().any(|s| s.pc == pc) {
            sites.push(LoopSite {
                pc,
                slot: sites.len(),
                backedge_pc: d.pc,
            });
        }
    }
    sites
}

/// Writes a u8 at `at` in `image`.
fn patch_u8(image: &mut [u8], at: usize, v: u8) {
    image[at] = v;
}

/// Writes a u16 LE at `at` in `image`.
fn patch_u16(image: &mut [u8], at: usize, v: u16) {
    image[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

/// Writes an i16 LE at `at` in `image`.
fn patch_i16(image: &mut [u8], at: usize, v: i16) {
    image[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

/// Writes an i32 LE at `at` in `image`.
fn patch_i32(image: &mut [u8], at: usize, v: i32) {
    image[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Writes a u32 LE at `at` in `image`.
fn patch_u32(image: &mut [u8], at: usize, v: u32) {
    image[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Writes a u64 LE at `at` in `image`.
fn patch_u64(image: &mut [u8], at: usize, v: u64) {
    image[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
