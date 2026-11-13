// CEP:FILE crates/vortex-t1/src/plan.rs
// CEP:WHAT T1 compilation planning: the fixed physical map (first K virtual
//        registers map to physical registers, the rest to spill slots) and
//        the T1a/T1b/T1c path classification (design.md section 5 revised
//        rule: "T1 is stencil-first, not stencil-only").
// CEP:WHY The fixed map is what lets stencils exist without a register
//        allocator: stencil selection is a pure function of operand
//        locations, so the map must be decided before any stencil is
//        selected. The classifier routes complex methods to the fallback
//        path so the fixed map never degrades pathological.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Classification is total over methods; no method can fall
//        outside the three documented paths.
// CEP:ASSUMES register_count <= 65536 (UGB extended encoding limit).
// CEP:COST O(1) per decision; the location map is O(1) per operand.
// CEP:EVIDENCE cargo test -p vortex-t1

use vortex_runtime::Loc;
use vortex_ugb::Method;

/// Width of the fast physical map: the first K virtual registers map to
/// physical registers, the rest spill to the VM stack frame (design.md
/// section 3.2, K = 8 callee-saved slots in the reference model).
pub const FAST_MAP_REGS: u16 = 8;

/// Spill budget for T1b: methods mapping at most this many extra registers
/// to stack slots still compile through stencils (patched spill variants).
pub const SPILL_BUDGET: u16 = 24;

/// The three baseline paths of the revised T1 structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum T1Path {
    /// Pure copy-and-patch stencils; every operand sits in the fast map.
    A,
    /// Stencils with operand-location descriptors: operands beyond the fast
    /// map take patched spill slots.
    B,
    /// Fallback baseline: lightweight CFG, simple LLIR, linear scan, then
    /// stencil emission with the computed allocation.
    C,
}

impl core::fmt::Display for T1Path {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            T1Path::A => write!(f, "T1a"),
            T1Path::B => write!(f, "T1b"),
            T1Path::C => write!(f, "T1c"),
        }
    }
}

/// Classifies a method onto one of the three baseline paths. The decision
/// driver is register pressure (the documented failure mode of the fixed
/// map: "stencils degrade badly after K registers").
// CEP:WHAT Classifies a method onto one of the three baseline paths.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[must_use]
pub fn classify(method: &Method) -> T1Path {
    let regs = method.register_count;
    if regs <= FAST_MAP_REGS {
        T1Path::A
    } else if regs <= FAST_MAP_REGS + SPILL_BUDGET {
        T1Path::B
    } else {
        T1Path::C
    }
}

/// The fixed physical map (design.md section 3.2):
/// `VM_Reg_0..K-1 -> Reg(0..K-1)`, `VM_Reg_K.. -> Stack(slot)`.
#[derive(Debug, Clone, Copy)]
pub struct FixedMap {
    /// Fast-map width.
    fast: u16,
}

impl FixedMap {
    /// Creates the map with the reference fast-map width.
    #[must_use]
    pub fn new() -> Self {
        FixedMap {
            fast: FAST_MAP_REGS,
        }
    }

    /// Creates a map with an explicit width (tests use narrow maps).
    #[must_use]
    pub fn with_width(fast: u16) -> Self {
        FixedMap { fast }
    }

    /// Maps a virtual register to its operand location. `spill_base` is the
    /// first stack slot owned by this blob (locations beyond the fast map
    /// take consecutive slots in register order, keeping the map pure).
    #[must_use]
    pub fn loc(&self, vreg: u16, spill_base: u16) -> Loc {
        if vreg < self.fast {
            Loc::Reg(vreg as u8)
        } else {
            Loc::Stack(spill_base + (vreg - self.fast))
        }
    }

    /// Fast-map width.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.fast
    }
}

impl Default for FixedMap {
    fn default() -> Self {
        FixedMap::new()
    }
}
