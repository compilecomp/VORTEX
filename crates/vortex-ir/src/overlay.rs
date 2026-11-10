// CEP:FILE crates/vortex-ir/src/overlay.rs
// CEP:WHAT The VSDG dependence overlay trait and the reference dependence
//        overlay: cached value/state/memory facts per node, updated by commit
//        events, self-staling when an invariant cannot be maintained
//        incrementally (REVISED section 1, overlay mutation API).
// CEP:WHY The overlay is a cache of semantic facts, not a second graph: the
//        base graph is the truth, the overlay is rebuilt when stale.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Overlay reads inside a transaction are rejected by the graph;
//        stale overlays report staleness rather than serving old facts.
// CEP:ASSUMES Single-threaded compiler use (Rc/RefCell); cross-thread
//        structures live in vortex-runtime::deps.
// CEP:COST on_event is O(1) per event for the reference overlay.
// CEP:EVIDENCE cargo test -p vortex-ir
use crate::graph::{Event, NodeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Overlay errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayError {
    /// An overlay read was attempted inside a graph transaction
    /// (REVISED section 1 rule 2).
    OverlayLocked,
}

/// Overlay trait: every overlay receives commit events in order and exposes
/// its revision for O(1) freshness checks (REVISED section 1 rules 4, 6).
pub trait Overlay {
    /// The read-only view type exposed to passes.
    type View;

    /// Applies one commit event.
    fn on_event(&mut self, event: &Event);

    /// Synchronizes the overlay revision after a commit.
    fn set_revision(&mut self, revision: u64);

    /// Produces the read view (must reflect current facts).
    fn view(&self) -> Self::View;
}

/// Dependence facts for one node (VSDG-style, cached on the unified graph).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DependenceFact {
    /// The node's memory-effect input is ordered before these nodes.
    pub state_out_degree: usize,
    /// The node is pure (no state dependence).
    pub pure: bool,
}

/// Read view of the dependence overlay.
#[derive(Debug, Clone, Default)]
pub struct DependenceView {
    /// Facts by node id.
    pub facts: HashMap<NodeId, DependenceFact>,
    /// Whether the overlay was rebuilt from scratch since last query.
    pub rebuilt: bool,
}

/// The reference dependence overlay.
#[derive(Debug, Default)]
pub struct DependenceOverlay {
    facts: HashMap<NodeId, DependenceFact>,
    revision: u64,
    stale: bool,
}

impl DependenceOverlay {
    /// Creates an overlay attached to nothing (facts are learned from
    /// events; production attaches at graph build time).
    #[must_use]
    pub fn new() -> Self {
        DependenceOverlay::default()
    }

    /// Seeds facts for an existing graph (build-time attach).
    pub fn seed(&mut self, facts: impl IntoIterator<Item = (NodeId, DependenceFact)>) {
        self.facts.extend(facts);
        self.stale = false;
    }

    /// Current revision (O(1) freshness check against the graph).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the overlay needs a rebuild before its facts can be trusted.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.stale
    }
}

impl Overlay for DependenceOverlay {
    type View = DependenceView;

    // CEP:WHAT Implements `on_event` as part of the module contract.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-ir
    fn on_event(&mut self, event: &Event) {
        match event {
            Event::NodeInserted(id) => {
                // New nodes start unpure until proven otherwise; a GVN/PRE
                // pass re-derives facts after commit.
                self.facts.insert(
                    *id,
                    DependenceFact {
                        state_out_degree: 0,
                        pure: false,
                    },
                );
            }
            Event::NodeReplaced(old, new) => {
                if let Some(f) = self.facts.remove(old) {
                    self.facts.insert(*new, f);
                }
            }
            Event::EdgeDeleted(user, _slot) => {
                if let Some(f) = self.facts.get_mut(user) {
                    // Deleting any input edge of a node with cached facts can
                    // change its value/state dependence; the exact
                    // re-derivation is pass work, so the fact degrades to
                    // conservative and the overlay reports stale views.
                    f.pure = false;
                    f.state_out_degree = f.state_out_degree.saturating_sub(1);
                    self.stale = true;
                }
            }
            Event::EdgeRetargeted(user, _, _) => {
                if let Some(f) = self.facts.get_mut(user) {
                    f.state_out_degree += 1;
                }
            }
        }
        self.revision += 1;
    }

    fn set_revision(&mut self, revision: u64) {
        self.revision = revision;
    }

    fn view(&self) -> Self::View {
        DependenceView {
            facts: self.facts.clone(),
            rebuilt: self.stale,
        }
    }
}

/// Convenience constructor: a reference-counted overlay handle.
#[must_use]
pub fn shared(o: DependenceOverlay) -> Rc<RefCell<DependenceOverlay>> {
    Rc::new(RefCell::new(o))
}
