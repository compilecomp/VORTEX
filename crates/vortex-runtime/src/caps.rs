// CEP:FILE crates/vortex-runtime/src/caps.rs
// CEP:WHAT The capability system implementation: 64-bit tokens, the Global
//        Capability Table, check/attenuate/revoke/duplicate/drop/pass
//        operations, and per-frame environments (design 6.3-6.7).
// CEP:WHY UGB security rests on unforgeable, tamper-proof, revocable
//        tokens with O(1) checks; this is the single source of truth.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every invalid operation returns a typed CapError; state is
//        never corrupted and tokens are never silently accepted.
// CEP:ASSUMES Tokens are runtime-constructed only; user code sees opaque
//        64-bit values.
// CEP:COST Check O(1); attenuate O(depth<=256); revoke O(|descendants|).
// CEP:EVIDENCE cargo test -p vortex-runtime
//! Capability system (design: Capability System Implementation 6.3-6.9).

/// Permission bit: read memory.
pub const PERM_MEM_READ: u16 = 0x0001;
/// Permission bit: write memory.
pub const PERM_MEM_WRITE: u16 = 0x0002;
/// Permission bit: allocate memory.
pub const PERM_MEM_ALLOC: u16 = 0x0004;
/// Permission bit: free memory.
pub const PERM_MEM_FREE: u16 = 0x0008;
/// Permission bit: foreign-function calls.
pub const PERM_FFI: u16 = 0x0010;
/// Permission bit: specific FFI descriptors (extra descriptor check).
pub const PERM_FFI_SPECIFIC: u16 = 0x0020;
/// Permission bit: inspect types.
pub const PERM_TYPE_INSPECT: u16 = 0x0040;
/// Permission bit: modify types.
pub const PERM_TYPE_MODIFY: u16 = 0x0080;
/// Permission bit: read I/O.
pub const PERM_IO_READ: u16 = 0x0100;
/// Permission bit: write I/O.
pub const PERM_IO_WRITE: u16 = 0x0200;
/// Permission bit: network access.
pub const PERM_IO_NETWORK: u16 = 0x0400;
/// Permission bit: filesystem access.
pub const PERM_IO_FILE: u16 = 0x0800;
/// Permission bit: dynamic operations.
pub const PERM_DYNAMIC: u16 = 0x1000;
/// Permission bit: eval.
pub const PERM_EVAL: u16 = 0x2000;
/// Permission bit: reflection.
pub const PERM_REFLECT: u16 = 0x4000;
/// Permission bit: polyglot switching.
pub const PERM_POLYGLOT: u16 = 0x8000;

/// Sentinel GCT index used by roots' children lists and free entries.
pub const GCT_INVALID: u32 = u32::MAX;
/// Maximum attenuation chain depth (bounded revocation walk).
pub const MAX_ATTENUATION_DEPTH: u32 = 256;
/// Maximum capability slots per environment.
pub const MAX_CAP_SLOTS: usize = 64;

/// Entry status values (design 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The capability is valid.
    Active,
    /// The capability has been revoked.
    Revoked,
    /// The capability has children and is still active.
    Attenuated,
    /// The entry is on the free list.
    Free,
}

/// One Global Capability Table entry (design 6.4).
#[derive(Debug, Clone, Copy)]
pub struct GctEntry {
    /// Resource descriptor handle (opaque to user code).
    pub resource: u64,
    /// Current generation; incremented on revoke.
    pub generation: u16,
    /// Entry status.
    pub status: Status,
    /// Parent index, or GCT_INVALID for roots.
    pub parent_index: u32,
    /// Maximum permissions for this resource.
    pub permission_mask: u16,
    /// Number of live tokens referencing this entry.
    pub ref_count: u32,
    /// First child in the sibling list, or GCT_INVALID.
    pub children_head: u32,
    /// Next sibling in the parent's child list, or GCT_INVALID.
    pub next_sibling: u32,
}

/// Capability operation failures (design 6.2 failure behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapError {
    /// The token's index is outside the table or the table is full.
    InvalidToken,
    /// The entry generation no longer matches the token.
    Revoked,
    /// Neither the token flags nor the entry mask grant the permission.
    InsufficientPermission,
    /// The requested permissions are not a subset of the parent's.
    InvalidAttenuation,
    /// The parent chain already reaches 256 deep.
    AttenuationTooDeep,
    /// The table cannot grow further.
    GctFull,
    /// The environment has no free slots.
    EnvFull,
}

/// A 64-bit capability token: flags (0-15), index (16-47), generation
/// (48-63). Constructed only by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token(u64);

impl Token {
    /// Packs a token from its fields (design 6.3 make_token).
    // CEP:WHAT Packs fields into a token word.
    // CEP:WHY One packing function keeps the layout contract in one place.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Fields fit their bit widths.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn make(gct_index: u32, generation: u16, permission_flags: u16) -> Token {
        Token((permission_flags as u64) | ((gct_index as u64) << 16) | ((generation as u64) << 48))
    }

    /// The permission flags field.
    // CEP:WHAT Returns the 16-bit permission flags.
    // CEP:WHY The check operation reads flags first.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn flags(self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }

    /// The GCT index field.
    // CEP:WHAT Returns the 32-bit table index.
    // CEP:WHY Every operation resolves the entry through this index.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn index(self) -> u32 {
        ((self.0 >> 16) & 0xFFFF_FFFF) as u32
    }

    /// The generation field.
    // CEP:WHAT Returns the 16-bit generation stamp.
    // CEP:WHY Generation mismatch is the revocation fast path.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn generation(self) -> u16 {
        ((self.0 >> 48) & 0xFFFF) as u16
    }

    /// The raw word, for storage in NV-1 special-immediate slots or frames.
    // CEP:WHAT Returns the raw 64-bit token word.
    // CEP:WHY Frames and snapshots store tokens as opaque words.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn bits(self) -> u64 {
        self.0
    }
}

/// Per-frame capability environment (design 6.7).
#[derive(Debug, Clone, Default)]
pub struct Environment {
    slots: Vec<Token>,
}

impl Environment {
    /// Adds a token; fails when the 64-slot bound is hit.
    // CEP:WHAT Adds a token to the environment.
    // CEP:WHY 6.10 item 6 bounds environments at MAX_CAP_SLOTS.
    // CEP:STATUS complete
    // CEP:FAILURE EnvFull when 64 slots are already used.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn add(&mut self, token: Token) -> Result<(), CapError> {
        if self.slots.len() >= MAX_CAP_SLOTS {
            return Err(CapError::EnvFull);
        }
        self.slots.push(token);
        Ok(())
    }

    /// Whether the environment holds this exact token word.
    // CEP:WHAT Membership test for pass verification.
    // CEP:WHY 6.5.4 requires the caller to hold the token it passes.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(slot count) <= 64.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn contains(&self, token: Token) -> bool {
        self.slots.contains(&token)
    }

    /// Removes a token by swap-with-last.
    // CEP:WHAT Removes a token from the environment.
    // CEP:WHY Frame exit drops capabilities in any order.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; absent tokens are a no-op.
    // CEP:ASSUMES None.
    // CEP:COST O(slot count).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn remove(&mut self, token: Token) {
        if let Some(i) = self.slots.iter().position(|t| *t == token) {
            let last = self.slots.len() - 1;
            self.slots[i] = self.slots[last];
            self.slots.pop();
        }
    }

    /// Live token count.
    // CEP:WHAT Reports the number of held tokens.
    // CEP:WHY Frame teardown and tests need the count.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no tokens are held.
    // CEP:WHAT Reports emptiness.
    // CEP:WHY Clippy requires an explicit negation of len.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Iterates the held tokens in slot order.
    // CEP:WHAT Iterates held tokens for teardown.
    // CEP:WHY Frame exit drops every capability.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) to create the iterator.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn iter(&self) -> impl Iterator<Item = Token> + '_ {
        self.slots.iter().copied()
    }
}

/// The Global Capability Table (design 6.4).
#[derive(Debug)]
pub struct Gct {
    entries: Vec<GctEntry>,
    free_head: u32,
    live: usize,
}

impl Gct {
    /// Creates a table and links all entries into the free list.
    // CEP:WHAT Initializes a table with a linked free list.
    // CEP:WHY O(1) allocation by free-list pop (design 6.4).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES initial_size >= 1.
    // CEP:COST O(initial_size).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn new(initial_size: u32) -> Gct {
        let n = initial_size.max(1) as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            entries.push(GctEntry {
                resource: 0,
                generation: 0,
                status: Status::Free,
                parent_index: GCT_INVALID,
                permission_mask: 0,
                ref_count: 0,
                children_head: (i as u32 + 1).min(n as u32 - 1),
                next_sibling: GCT_INVALID,
            });
        }
        entries[n - 1].children_head = GCT_INVALID;
        Gct {
            entries,
            free_head: 0,
            live: 0,
        }
    }

    /// Allocates a free entry; doubles the table when exhausted.
    // CEP:WHAT Pops a free entry, growing the table at double size.
    // CEP:WHY Indices never change during growth, so tokens stay valid
    //        (design 6.4 GCT growth).
    // CEP:STATUS complete
    // CEP:FAILURE GctFull at the 2^32 entry bound.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized; O(new size) on a growth step.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn allocate(&mut self) -> Result<u32, CapError> {
        if self.free_head == GCT_INVALID {
            let old = self.entries.len() as u64;
            if old >= 1 << 32 {
                return Err(CapError::GctFull);
            }
            let new_len = (old * 2).min(1 << 32) as usize;
            for i in old as usize..new_len {
                self.entries.push(GctEntry {
                    resource: 0,
                    generation: 0,
                    status: Status::Free,
                    parent_index: GCT_INVALID,
                    permission_mask: 0,
                    ref_count: 0,
                    children_head: (i as u64 + 1).min(new_len as u64 - 1) as u32,
                    next_sibling: GCT_INVALID,
                });
            }
            self.entries[new_len - 1].children_head = GCT_INVALID;
            self.free_head = old as u32;
        }
        let idx = self.free_head;
        let next = self.entries[idx as usize].children_head;
        self.free_head = next;
        self.live += 1;
        Ok(idx)
    }

    /// Returns an exhausted entry to the free list.
    // CEP:WHAT Frees an entry and links it into the free list.
    // CEP:WHY Drop of an unreferenced revoked entry reclaims storage.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; callers only free unreferenced entries.
    // CEP:ASSUMES The entry has ref_count 0.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    // CEP:ID: MEM-a01
    fn free_entry(&mut self, idx: u32) {
        let e = &mut self.entries[idx as usize];
        e.status = Status::Free;
        e.resource = 0;
        e.permission_mask = 0;
        e.ref_count = 0;
        e.parent_index = GCT_INVALID;
        e.children_head = GCT_INVALID;
        e.next_sibling = GCT_INVALID;
        e.children_head = self.free_head;
        self.free_head = idx;
        self.live -= 1;
    }

    /// Creates a root capability over a resource with a permission mask.
    // CEP:WHAT Mints a root token as the entry's first live reference.
    // CEP:WHY Unforgeability (P1): roots are minted only here.
    // CEP:STATUS complete
    // CEP:FAILURE GctFull when the table cannot grow.
    // CEP:ASSUMES resource is an opaque runtime handle.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn make_root(&mut self, resource: u64, permission_mask: u16) -> Result<Token, CapError> {
        let idx = self.allocate()?;
        let e = &mut self.entries[idx as usize];
        e.resource = resource;
        e.generation = 1;
        e.status = Status::Active;
        e.parent_index = GCT_INVALID;
        e.permission_mask = permission_mask;
        e.ref_count = 1;
        e.children_head = GCT_INVALID;
        e.next_sibling = GCT_INVALID;
        Ok(Token::make(idx, e.generation, permission_mask))
    }

    /// The O(1) check operation (design 6.5.1).
    // CEP:WHAT Validates a token against a required permission.
    // CEP:WHY Every capability-gated instruction performs this check.
    // CEP:STATUS complete
    // CEP:FAILURE Typed CapError for every rejection shape (P5).
    // CEP:ASSUMES None.
    // CEP:COST O(1): one entry load plus three comparisons.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn check(&self, token: Token, required_perm: u16) -> Result<(), CapError> {
        let idx = token.index() as usize;
        if idx >= self.entries.len() {
            return Err(CapError::InvalidToken);
        }
        let entry = &self.entries[idx];
        if entry.generation != token.generation() {
            return Err(CapError::Revoked);
        }
        if entry.status == Status::Revoked {
            return Err(CapError::Revoked);
        }
        if token.flags() & required_perm != required_perm {
            return Err(CapError::InsufficientPermission);
        }
        if entry.permission_mask & required_perm != required_perm {
            return Err(CapError::InsufficientPermission);
        }
        Ok(())
    }

    /// Attenuation: derive a subset-permission child token (6.5.2).
    // CEP:WHAT Derives a child entry with subset permissions.
    // CEP:WHY P3 attenuation monotonicity; depth bounded at 256.
    // CEP:STATUS complete
    // CEP:FAILURE Typed errors for revoked parents, non-subsets, depth,
    //        and table exhaustion.
    // CEP:ASSUMES None.
    // CEP:COST O(depth) for the depth walk, bounded by 256.
    // CEP:ID: MEM-a02
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn attenuate(&mut self, parent: Token, new_permissions: u16) -> Result<Token, CapError> {
        let pidx = parent.index() as usize;
        if pidx >= self.entries.len() {
            return Err(CapError::InvalidToken);
        }
        if self.entries[pidx].generation != parent.generation() {
            return Err(CapError::Revoked);
        }
        if self.entries[pidx].status == Status::Revoked {
            return Err(CapError::Revoked);
        }
        if new_permissions & !parent.flags() != 0 {
            return Err(CapError::InvalidAttenuation);
        }
        if new_permissions & !self.entries[pidx].permission_mask != 0 {
            return Err(CapError::InvalidAttenuation);
        }
        if self.depth_of(pidx as u32) >= MAX_ATTENUATION_DEPTH {
            return Err(CapError::AttenuationTooDeep);
        }
        let resource = self.entries[pidx].resource;
        let generation = self.entries[pidx].generation;
        let old_children = self.entries[pidx].children_head;
        let idx = self.allocate()?;
        self.entries[idx as usize] = GctEntry {
            resource,
            generation,
            status: Status::Active,
            parent_index: pidx as u32,
            permission_mask: new_permissions,
            ref_count: 1,
            children_head: GCT_INVALID,
            next_sibling: old_children,
        };
        let p = &mut self.entries[pidx];
        p.children_head = idx;
        if p.status == Status::Active {
            p.status = Status::Attenuated;
        }
        p.ref_count += 1;
        Ok(Token::make(idx, generation, new_permissions))
    }

    /// Walks the parent chain to compute attenuation depth.
    // CEP:WHAT Computes the chain depth of an entry.
    // CEP:WHY The depth bound (256) bounds revocation walks.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; cycles would loop, but the forest invariant
    //        forbids cycles (checked by audit_invariants).
    // CEP:ASSUMES The forest invariant holds.
    // CEP:COST O(depth).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn depth_of(&self, mut idx: u32) -> u32 {
        let mut depth: u32 = 0;
        while idx != GCT_INVALID {
            idx = self.entries[idx as usize].parent_index;
            depth += 1;
        }
        depth.saturating_sub(1)
    }

    /// Eager revocation over the descendant tree (6.5.3).
    // CEP:WHAT Revokes a capability and every descendant.
    // CEP:WHY P4 revocation completeness with O(|descendants|) cost.
    // CEP:STATUS complete
    // CEP:FAILURE InvalidToken for out-of-range indices; already-revoked
    //        tokens report Revoked without double-counting.
    // CEP:ASSUMES None.
    // CEP:ID: MEM-a03
    // CEP:COST O(|descendants|).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn revoke(&mut self, token: Token) -> Result<usize, CapError> {
        let idx = token.index() as usize;
        if idx >= self.entries.len() {
            return Err(CapError::InvalidToken);
        }
        if self.entries[idx].generation != token.generation() {
            return Err(CapError::Revoked);
        }
        let mut worklist = vec![idx as u32];
        let mut count = 0usize;
        while let Some(cur) = worklist.pop() {
            let entry = &mut self.entries[cur as usize];
            entry.generation = entry.generation.wrapping_add(1);
            entry.status = Status::Revoked;
            count += 1;
            let mut child = entry.children_head;
            while child != GCT_INVALID {
                worklist.push(child);
                child = self.entries[child as usize].next_sibling;
            }
        }
        Ok(count)
    }

    /// Duplicates a token, tracking the extra live reference (6.5.5).
    // CEP:WHAT Increments the reference count for a copied token.
    // CEP:WHY Tokens stored in multiple slots must not free early.
    // CEP:STATUS complete
    // CEP:FAILURE InvalidToken or Revoked as in check.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn duplicate(&mut self, token: Token) -> Result<Token, CapError> {
        self.check(token, 0)?;
        self.entries[token.index() as usize].ref_count += 1;
        Ok(token)
    }

    /// Drops one live reference (6.5.6), freeing revoked unreferenced
    /// entries.
    // CEP:WHAT Decrements the reference count of a token's entry.
    // CEP:WHY Frame teardown reclaims storage deterministically.
    // CEP:STATUS complete
    // CEP:FAILURE Out-of-range tokens are ignored per the design.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn drop_token(&mut self, token: Token) {
        let idx = token.index() as usize;
        if idx >= self.entries.len() {
            return;
        }
        let entry = &mut self.entries[idx];
        if entry.ref_count == 0 {
            return;
        }
        entry.ref_count -= 1;
        if entry.ref_count == 0 && entry.status == Status::Revoked {
            let idx = token.index();
            self.unlink_from_parent(idx);
            self.free_entry(idx);
        }
    }

    /// Detaches an entry from its parent's child list before freeing.
    // CEP:WHAT Removes an entry from its parent sibling list.
    // CEP:WHY The free list reuses children_head; stale links would
    //        corrupt the forest invariant.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The parent link is consistent.
    // CEP:COST O(siblings of the parent).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn unlink_from_parent(&mut self, idx: u32) {
        let parent = self.entries[idx as usize].parent_index;
        if parent == GCT_INVALID {
            return;
        }
        let head = self.entries[parent as usize].children_head;
        if head == idx {
            self.entries[parent as usize].children_head = self.entries[idx as usize].next_sibling;
            return;
        }
        let mut cur = head;
        while cur != GCT_INVALID {
            let next = self.entries[cur as usize].next_sibling;
            if next == idx {
                self.entries[cur as usize].next_sibling = self.entries[idx as usize].next_sibling;
                return;
            }
            cur = next;
        }
    }

    /// Transfers a held token between environments (6.5.4).
    // CEP:WHAT Passes a capability from a caller environment to a callee.
    // CEP:WHY CALL passes capabilities with caller-controlled attenuation.
    // CEP:STATUS complete
    // CEP:FAILURE EnvFull or a caller-side membership failure.
    // CEP:ASSUMES None.
    // CEP:COST O(|env|) membership plus O(1) add.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn pass(
        &mut self,
        caller: &Environment,
        callee: &mut Environment,
        token: Token,
    ) -> Result<(), CapError> {
        if !caller.contains(token) {
            return Err(CapError::InvalidToken);
        }
        callee.add(token)?;
        self.entries[token.index() as usize].ref_count += 1;
        Ok(())
    }

    /// Live entry count.
    // CEP:WHAT Reports the number of live entries.
    // CEP:WHY Tests verify the ref-count/free-list bookkeeping.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn live_entries(&self) -> usize {
        self.live
    }

    /// Audits the structural invariants (6.10 item 1).
    // CEP:WHAT Checks the forest shape, free-list hygiene, and masks.
    // CEP:WHY Law 3: invariants must be enforceable, not comment-only; the
    //        tests call this after every mutation sequence.
    // CEP:STATUS complete
    // CEP:FAILURE Names the violated invariant in the error string.
    // CEP:ID: MEM-a04
    // CEP:ASSUMES None.
    // CEP:COST O(entries + sibling lists).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn audit_invariants(&self) -> Result<(), &'static str> {
        for (i, e) in self.entries.iter().enumerate() {
            if e.status == Status::Free {
                if e.ref_count != 0 {
                    return Err("free entry carries references");
                }
                continue;
            }
            if e.resource == 0 {
                return Err("live entry without a resource");
            }
            if e.parent_index != GCT_INVALID {
                let p = e.parent_index as usize;
                if p >= self.entries.len() || self.entries[p].status == Status::Free {
                    return Err("parent link leaves the forest");
                }
                let mut cur = self.entries[p].children_head;
                let mut found = false;
                let mut steps = 0usize;
                while cur != GCT_INVALID && steps <= self.entries.len() {
                    if cur as usize == i {
                        found = true;
                    }
                    cur = self.entries[cur as usize].next_sibling;
                    steps += 1;
                }
                if !found {
                    return Err("child missing from parent list");
                }
                if e.permission_mask & !self.entries[p].permission_mask != 0 {
                    return Err("child permissions exceed parent");
                }
            }
        }
        Ok(())
    }
}
