// CEP:FILE crates/vortex-runtime/src/caps.rs
// CEP:WHAT Capability table with 16-bit version epochs: grant, revoke with
//        ancestor propagation, and the T0 fast-path check with site epoch
//        caches (UGB-1 section 6.3, UGB-INV-01/07).
// CEP:WHY Revocation must cost one increment and one compare at every
//        consumer site, with a stale cache able to cause only a slow path,
//        never a bypass.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Checks return SecurityError; revocation saturates at 0xFFFF
//        where the capability becomes permanently invalid.
// CEP:ASSUMES Epochs are per-capability and monotonically increasing.
// CEP:COST Fast path: two loads + compare. Slow path: delegation-chain walk.
// CEP:EVIDENCE cargo test -p vortex-runtime (epoch revocation tests)
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

/// Authority bits (UGB-1 section 2.1 authority mask space).
pub mod auth {
    /// Foreign function interface.
    pub const FFI: u64 = 1 << 0;
    /// Filesystem read.
    pub const FILE_READ: u64 = 1 << 1;
    /// Network access.
    pub const NETWORK: u64 = 1 << 2;
    /// Fast-math authorization (NUM-1 section 8, CAP_FAST_MATH).
    pub const FAST_MATH: u64 = 1 << 3;
    /// Direct syscall authorization.
    pub const SYSCALL: u64 = 1 << 4;
}

/// One capability table entry.
#[derive(Debug)]
pub struct CapEntry {
    /// Authority mask.
    pub authority: u64,
    /// Resource handle (fd, pointer, engine object id).
    pub resource: u64,
    /// Version epoch: incremented on revocation (saturating).
    pub epoch: AtomicU16,
    /// Revoked flag.
    pub revoked: AtomicBool,
    /// Delegation parent (revocation propagates up the chain).
    pub parent: Option<u32>,
}

/// The capability table.
#[derive(Debug, Default)]
pub struct CapabilityTable {
    entries: Vec<CapEntry>,
}

/// A capability handle: index into the table. The engine never hands out raw
/// table indices to guest code; guest code sees NV-1 capability objects whose
/// payload word 1 is this index. For the reference runtime the index is the
/// handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapId(pub u32);

impl CapabilityTable {
    /// Grants a new capability with the given authority and resource.
    pub fn grant(&mut self, authority: u64, resource: u64, parent: Option<CapId>) -> CapId {
        let idx = self.entries.len();
        self.entries.push(CapEntry {
            authority,
            resource,
            epoch: AtomicU16::new(1),
            revoked: AtomicBool::new(false),
            parent: parent.map(|p| p.0),
        });
        CapId(idx as u32)
    }

    /// Revokes a capability and all ancestors: each affected entry's epoch is
    /// incremented (saturating) and its revoked flag set. One increment per
    /// revocation is the entire broadcast mechanism (UGB-1 section 6.3).
    // CEP:WHAT Revokes a capability and all ancestors: each affected entry's epoch is incremented (saturating) and its revoked flag set.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn revoke(&self, id: CapId) {
        let mut current = Some(id.0);
        while let Some(i) = current {
            let entry = &self.entries[i as usize];
            entry.revoked.store(true, Ordering::Release);
            let _ = entry
                .epoch
                .try_update(Ordering::AcqRel, Ordering::Acquire, |e: u16| {
                    Some(e.saturating_add(1))
                });
            current = entry.parent;
        }
    }

    /// The T0 fast-path check (UGB-1 section 6.3 step 1-4).
    ///
    /// `site` is the per-call-site epoch cache in the method's metadata. On
    /// epoch match and clear revoked flag, the check is two loads, one
    /// compare, and the authority bit test: no delegation walk, no full-mask
    /// validation. On mismatch, the slow path re-reads the table; a stale
    /// cache can only add slow paths, never bypass revocation (UGB-INV-07).
    // CEP:WHAT The T0 fast-path check (UGB-1 section 6.3 step 1-4).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn check_fast(
        &self,
        site: &mut crate::profile::EpochCache,
        id: CapId,
        req_auth: u64,
    ) -> Result<(), crate::trap::Trap> {
        let entry = self
            .entries
            .get(id.0 as usize)
            .ok_or(crate::trap::Trap::SecurityError)?;
        let epoch = entry.epoch.load(Ordering::Acquire);
        let revoked = entry.revoked.load(Ordering::Acquire);
        if !revoked && site.cached_epoch == Some(epoch) {
            return if entry.authority & req_auth == req_auth {
                Ok(())
            } else {
                Err(crate::trap::Trap::SecurityError)
            };
        }
        // Slow path: full validation, then refresh the cache.
        if revoked || entry.authority & req_auth != req_auth {
            return Err(crate::trap::Trap::SecurityError);
        }
        site.cached_epoch = Some(epoch);
        Ok(())
    }

    /// Snapshot of an entry's epoch for tests and diagnostics.
    #[must_use]
    pub fn epoch_of(&self, id: CapId) -> u16 {
        self.entries[id.0 as usize].epoch.load(Ordering::Acquire)
    }

    /// Whether an entry is revoked.
    #[must_use]
    pub fn is_revoked(&self, id: CapId) -> bool {
        self.entries[id.0 as usize].revoked.load(Ordering::Acquire)
    }
}
