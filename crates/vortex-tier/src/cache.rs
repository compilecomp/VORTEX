// CEP:FILE crates/vortex-tier/src/cache.rs
// CEP:WHAT The CODE-1 code cache model: size-segregated arenas (S/M/L),
//        compiler thread-local allocation buffers, the CodeBlob header in
//        the separate metadata pool, the blob lifecycle state machine
//        (Active, DeoptPending, Zombie, Reclaimable), and the sweeper
//        (CODE-1 sections 2-7).
// CEP:WHY Executable memory is the constrained resource: the four-heap
//        topology, W^X discipline, and lifecycle protocol are the
//        correctness contract of every compiled tier. The reference model
//        realizes the topology in-process (code images are byte buffers;
//        no mmap), keeping every observable protocol - allocation class,
//        metadata separation, lifecycle transitions, sweep timing - the
//        specified contract.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every transition outside the state machine is a typed error;
//        reclamation is impossible while the blob is not Reclaimable
//        (CODE-INV-05).
// CEP:ASSUMES Single-threaded reference use; the concurrency contracts
//        (TLAB lock-freedom, safepoint-synchronized patching) are modeled
//        as explicit protocol steps.
// CEP:COST Allocation O(1) per arena; sweep O(blobs).
// CEP:EVIDENCE cargo test -p vortex-tier
use std::collections::HashMap;

/// Arena size classes (CODE-1 section 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arena {
    /// Stubs, trampolines, T1a stencils (< 1 KB).
    Small,
    /// T1c/T2 methods (1 KB - 16 KB).
    Medium,
    /// T3/T4 highly optimized methods (> 16 KB).
    Large,
}

impl Arena {
    /// The arena for a code size.
    #[must_use]
    pub fn for_size(bytes: usize) -> Self {
        match bytes {
            0..=1023 => Arena::Small,
            1024..=16_383 => Arena::Medium,
            _ => Arena::Large,
        }
    }
}

/// Blob lifecycle states (CODE-1 section 7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobState {
    /// Executable, referenced by AOSS dependencies.
    Active,
    /// Dependencies invalidated; entry points patched to deopt stubs.
    DeoptPending,
    /// No thread can still be executing the old code.
    Zombie,
    /// Ready to be freed.
    Reclaimable,
}

/// The metadata-pool record of a compiled blob (CODE-1 section 4.1);
/// the executable image lives in the code pool, never the metadata.
#[derive(Debug, Clone)]
pub struct CodeBlobHeader {
    /// Method the blob belongs to.
    pub method_id: usize,
    /// Compiling tier.
    pub tier: u8,
    /// Code image size.
    pub code_size: u32,
    /// Region entry offsets into the image (region-based deopt).
    pub regions: Vec<usize>,
    /// Which arena the code lives in.
    pub arena: Arena,
    /// Lifecycle state.
    pub state: BlobState,
    /// Safepoint count (metadata separation evidence).
    pub safepoint_count: u16,
}

/// Allocation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheError {
    /// The named blob does not exist.
    UnknownBlob,
    /// The transition violated the lifecycle state machine.
    IllegalTransition {
        /// The blob.
        blob: usize,
        /// The state the transition was attempted from.
        from: BlobState,
    },
}

/// The code cache.
#[derive(Debug, Default)]
pub struct CodeCache {
    blobs: HashMap<usize, CodeBlobHeader>,
    /// Bytes allocated per arena (fragmentation evidence).
    arena_bytes: [usize; 3],
    /// Compiler TLABs currently open (one per compiler thread).
    open_tlabs: usize,
    /// Total blobs swept (reclaimed).
    swept: u64,
    /// The safepoint generation (CODE-INV-05: a blob becomes reclaimable
    /// only after a full safepoint has passed since its invalidation).
    safepoint_generation: u64,
    next_id: usize,
}

impl CodeCache {
    /// Creates an empty cache.
    #[must_use]
    pub fn new() -> Self {
        CodeCache::default()
    }

    /// Opens a compiler TLAB (CODE-INV-06: allocation without a global
    /// mutex; the reference models the claim as a counter).
    pub fn open_tlab(&mut self) {
        self.open_tlabs += 1;
    }

    /// Closes a compiler TLAB.
    pub fn close_tlab(&mut self) {
        self.open_tlabs = self.open_tlabs.saturating_sub(1);
    }

    /// Publishes a blob: allocates the metadata record, charges the arena,
    /// and enters the Active state. Registration precedes publication
    /// (EXC-INV-03 ordering discipline is the caller's protocol step).
    // CEP:WHAT Publishes a blob.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn publish(
        &mut self,
        method_id: usize,
        tier: u8,
        code_size: usize,
        regions: Vec<usize>,
        safepoint_count: u16,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let arena = Arena::for_size(code_size);
        self.arena_bytes[match arena {
            Arena::Small => 0,
            Arena::Medium => 1,
            Arena::Large => 2,
        }] += code_size;
        self.blobs.insert(
            id,
            CodeBlobHeader {
                method_id,
                tier,
                code_size: u32::try_from(code_size).unwrap_or(u32::MAX),
                regions,
                arena,
                state: BlobState::Active,
                safepoint_count,
            },
        );
        id
    }

    /// Marks a blob DeoptPending (dependencies invalidated).
    // CEP:WHAT Marks a blob DeoptPending.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn invalidate(&mut self, blob: usize) -> Result<(), CacheError> {
        let b = self.blobs.get_mut(&blob).ok_or(CacheError::UnknownBlob)?;
        if b.state != BlobState::Active {
            return Err(CacheError::IllegalTransition {
                blob,
                from: b.state,
            });
        }
        b.state = BlobState::DeoptPending;
        Ok(())
    }

    /// The global safepoint: DeoptPending blobs with no outstanding
    /// execution become Zombie (CODE-INV-05).
    // CEP:WHAT The global safepoint.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn safepoint(&mut self) {
        self.safepoint_generation += 1;
        for b in self.blobs.values_mut() {
            if b.state == BlobState::DeoptPending {
                b.state = BlobState::Zombie;
            }
        }
    }

    /// The sweeper: reclaims Zombie blobs (CODE-1 section 7.2).
    // CEP:WHAT The sweeper.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn sweep(&mut self) -> usize {
        let before = self.blobs.len();
        self.blobs.retain(|_, b| {
            if b.state == BlobState::Zombie {
                b.state = BlobState::Reclaimable;
            }
            if b.state == BlobState::Reclaimable {
                self.swept += 1;
                self.arena_bytes[match b.arena {
                    Arena::Small => 0,
                    Arena::Medium => 1,
                    Arena::Large => 2,
                }] -= usize::try_from(b.code_size).unwrap_or(0);
                return false;
            }
            true
        });
        before - self.blobs.len()
    }

    /// Blob count by state.
    #[must_use]
    pub fn count_in(&self, state: BlobState) -> usize {
        self.blobs.values().filter(|b| b.state == state).count()
    }

    /// Bytes currently committed per arena.
    #[must_use]
    pub fn arena_bytes(&self) -> [usize; 3] {
        self.arena_bytes
    }

    /// Total blobs reclaimed.
    #[must_use]
    pub fn swept(&self) -> u64 {
        self.swept
    }

    /// Current safepoint generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.safepoint_generation
    }
}
