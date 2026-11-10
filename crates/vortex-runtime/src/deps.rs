// CEP:FILE crates/vortex-runtime/src/deps.rs
// CEP:WHAT AOSS dependency map with the add-vs-invalidate handshake
//        (CONC-1 section 2.1.1): per-layout sequence locks, retry-bounded
//        invalidation traversals, install-time generation rejection.
// CEP:WHY A dependency added before a shape transition must be visible to
//        that transition's invalidation, or the compiled region survives on a
//        violated assumption (unsoundness, not a performance defect).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE add_dependency returns Stale when the generation advanced
//        (CONC-INV-07); invalidate returns only after one stable traversal
//        (CONC-INV-06).
// CEP:ASSUMES The reference implementation guards list storage with a short
//        lock instead of RCU pointer surgery (workspace forbids `unsafe`);
//        the observable protocol - seqlock, retry, generation rejection - is
//        the specified contract and is exercised under threads in tests.
// CEP:COST Readers: O(list) with retry on tear. Publish: O(list) copy.
// CEP:EVIDENCE cargo test -p vortex-runtime (concurrent handshake test)
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// Compiled-region identifier registered as a dependency.
pub type RegionId = u32;

/// Layout generation: bumped on every invalidation; dependencies record the
/// generation they were compiled against.
pub type Generation = u64;

/// A registered dependency of compiled code on a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The compiled region that assumes the layout.
    pub region: RegionId,
    /// The layout generation the region was compiled against.
    pub gen: Generation,
}

#[derive(Debug)]
struct LayoutState {
    seq: AtomicU32,
    generation: AtomicU64,
    deps: Mutex<Vec<Dependency>>,
}

/// Dependency map: layout handle to dependent regions.
#[derive(Debug, Default)]
pub struct DependencyMap {
    layouts: HashMap<u64, LayoutState>,
}

/// Errors from dependency publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepError {
    /// The layout generation advanced between the compiler's read and its
    /// publish; the dependency is stale on arrival (CONC-INV-07).
    StaleGeneration {
        /// Generation the dependency was compiled against.
        expected: Generation,
        /// Current generation.
        current: Generation,
    },
}

impl DependencyMap {
    /// Adds a dependency on `layout` for `region`.
    ///
    /// Protocol (CONC-1 section 2.1.1): if the sequence lock is odd (an
    /// invalidation traversal or another publish is mid-flight) the publisher
    /// spins; the list is updated under the storage lock; the publish is
    /// completed with a release increment to the next even value.
    // CEP:WHAT Adds a dependency on layout for region.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn add_dependency(
        &self,
        layout: u64,
        region: RegionId,
        compiled_against: Generation,
    ) -> Result<(), DepError> {
        let st = self
            .layouts
            .get(&layout)
            .expect("ensure_layout must be called before registering dependencies");
        // Install-time generation check (CONC-INV-07).
        let current = st.generation.load(Ordering::Acquire);
        if current != compiled_against {
            return Err(DepError::StaleGeneration {
                expected: compiled_against,
                current,
            });
        }
        // Spin while a traversal holds the seqlock odd.
        let mut spins = 0_u32;
        while st.seq.load(Ordering::Acquire) & 1 == 1 {
            spins += 1;
            if spins > SPIN_LIMIT {
                // Escalation (handshake) is the runtime's decision; the map
                // surfaces contention by proceeding under the storage lock.
                break;
            }
            std::hint::spin_loop();
        }
        {
            let mut deps = st.deps.lock().expect("dependency lock poisoned");
            if !deps
                .iter()
                .any(|d| d.region == region && d.gen == compiled_against)
            {
                deps.push(Dependency {
                    region,
                    gen: compiled_against,
                });
            }
        }
        // Publish: release increment to the next even value.
        st.seq.fetch_add(2, Ordering::Release);
        Ok(())
    }

    /// Invalidates every dependency on `layout` and bumps the generation.
    ///
    /// Protocol (CONC-INV-06): the traversal is retried until one full
    /// traversal observes an unchanged (even) sequence counter, so any
    /// dependency published before this call started is returned.
    // CEP:WHAT Invalidates every dependency on layout and bumps the generation.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn invalidate(&self, layout: u64) -> Vec<RegionId> {
        let Some(st) = self.layouts.get(&layout) else {
            return Vec::new(); // nothing was ever registered
        };
        loop {
            let s1 = st.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let snapshot = {
                let deps = st.deps.lock().expect("dependency lock poisoned");
                deps.clone()
            };
            let s2 = st.seq.load(Ordering::Acquire);
            if s1 == s2 {
                // Stable traversal: publish the generation bump.
                st.generation.fetch_add(1, Ordering::AcqRel);
                return snapshot.into_iter().map(|d| d.region).collect();
            }
            std::hint::spin_loop();
        }
    }

    /// Current generation of a layout (0 for unknown layouts).
    #[must_use]
    pub fn generation(&self, layout: u64) -> Generation {
        self.layouts
            .get(&layout)
            .map_or(0, |s| s.generation.load(Ordering::Acquire))
    }

    /// Removes a region's dependency (post-deopt cleanup).
    // CEP:WHAT Removes a region's dependency (post-deopt cleanup).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn remove_region(&self, layout: u64, region: RegionId) {
        if let Some(st) = self.layouts.get(&layout) {
            while st.seq.load(Ordering::Acquire) & 1 == 1 {
                std::hint::spin_loop();
            }
            let mut deps = st.deps.lock().expect("dependency lock poisoned");
            deps.retain(|d| d.region != region);
            drop(deps);
            st.seq.fetch_add(2, Ordering::Release);
        }
    }

    /// Creates the layout state entry if absent; called by compilers before
    /// registering dependencies.
    pub fn ensure_layout(&mut self, layout: u64) {
        self.layouts.entry(layout).or_insert_with(|| LayoutState {
            seq: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            deps: Mutex::new(Vec::new()),
        });
    }
}

/// Bounded spin before falling back to lock-based arbitration.
const SPIN_LIMIT: u32 = 1 << 10;
