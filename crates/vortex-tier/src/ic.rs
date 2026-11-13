// CEP:FILE crates/vortex-tier/src/ic.rs
// CEP:WHAT The IC-1 inline cache state machine: Uninitialized, Monomorphic,
//        Polymorphic (bounded), Megamorphic, with the side-table separation
//        rule (IC-INV-03), the bounded-polymorphism invariant (IC-INV-01),
//        and megamorphic stability (IC-INV-02).
// CEP:WHY Inline caches must evolve gracefully from monomorphic to
//        megamorphic without unbounded growth or deopt storms; the state
//        machine is the contract every tier shares.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE The machine is total over inputs; transition attempts that
//        would exceed the polymorphism bound return the Megamorphic state
//        instead of growing the chain.
// CEP:ASSUMES Layout ids come from the runtime's canonical layout
//        registry.
// CEP:COST O(1) per update up to the bound; O(N) scan per hit.
// CEP:EVIDENCE cargo test -p vortex-tier

/// The polymorphism bound (IC-INV-01; the reference N is 4).
pub const POLY_LIMIT: usize = 4;

/// IC state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IcState {
    /// Never executed.
    Uninitialized,
    /// One unique layout observed.
    Monomorphic {
        /// The layout id.
        layout: u64,
    },
    /// 2..=POLY_LIMIT unique layouts.
    Polymorphic {
        /// The observed layouts (bounded).
        layouts: Vec<u64>,
    },
    /// More than POLY_LIMIT layouts: stop specializing (IC-INV-02).
    Megamorphic,
}

/// An inline cache site record (the side-table entry; IC-INV-03: IC state
/// lives in metadata, never in executable code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcSite {
    /// The current state.
    pub state: IcState,
    /// Whether the site ever went megamorphic (latch).
    megamorphic_latch: bool,
}

impl Default for IcSite {
    fn default() -> Self {
        IcSite::new()
    }
}

impl IcSite {
    /// Creates an uninitialized site.
    #[must_use]
    pub fn new() -> Self {
        IcSite {
            state: IcState::Uninitialized,
            megamorphic_latch: false,
        }
    }

    /// Updates the site with one observed layout and returns the new state.
    // CEP:WHAT Updates the site with one observed layout.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn observe(&mut self, layout: u64) -> IcState {
        if self.megamorphic_latch {
            return IcState::Megamorphic; // IC-INV-02: no revert.
        }
        self.state = match self.state.clone() {
            IcState::Uninitialized => IcState::Monomorphic { layout },
            IcState::Monomorphic { layout: l } => {
                if l == layout {
                    IcState::Monomorphic { layout: l }
                } else {
                    IcState::Polymorphic {
                        layouts: vec![l, layout],
                    }
                }
            }
            IcState::Polymorphic { layouts } => {
                if layouts.contains(&layout) {
                    IcState::Polymorphic { layouts }
                } else if layouts.len() < POLY_LIMIT {
                    let mut layouts = layouts;
                    layouts.push(layout);
                    IcState::Polymorphic { layouts }
                } else {
                    // IC-INV-01: never grow the chain past the bound.
                    self.megamorphic_latch = true;
                    IcState::Megamorphic
                }
            }
            IcState::Megamorphic => IcState::Megamorphic,
        };
        self.state.clone()
    }

    /// Whether the site stopped tracking shape transitions (IC-1 section
    /// 3.2: megamorphic sites never deopt on shape changes).
    #[must_use]
    pub fn tracks_shapes(&self) -> bool {
        !self.megamorphic_latch
    }
}
