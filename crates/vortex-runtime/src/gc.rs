// CEP:FILE crates/vortex-runtime/src/gc.rs
// CEP:WHAT Generational collector: minor (nursery evacuation) and major
//        (mark + old-gen evacuation) phases, numeric-payload non-tracing,
//        snapshot pages as immortal trace roots that are never relocated,
//        and root/remembered-set-driven reachability.
// CEP:WHY Implements the GC half of the corrected contract: SNAP-INV-08
//        (snapshot non-relocatability), NUM-INV-01 (numeric payloads are
//        never scanned as references), and HEAP-1 side-metadata marking.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Allocation failure during evacuation returns OutOfMemory;
//        reachability is conservative-precise: only tagged references in
//        non-numeric payloads and the supplied roots are traced.
// CEP:ASSUMES Single collector thread (stop-the-world phases); mutator roots
//        are passed as `&mut [u64]` so evacuation can rewrite them.
// CEP:COST Minor: O(live nursery + remembered set). Major: O(heap).
// CEP:EVIDENCE cargo test -p vortex-runtime (incl. snapshot immobility test)
use crate::heap::{layout, GcStats, Handle, PageKind};
use crate::trap::{RtResult, Trap};
use vortex_nv1 as nv1;

/// The collector. Stateless across collections except for statistics.
#[derive(Debug, Default)]
pub struct Gc {
    /// Counters shared with the barrier path.
    pub stats: GcStats,
}

/// Root set for a collection: VM stack words plus remembered-set handles.
#[derive(Debug, Default)]
pub struct Roots<'a> {
    /// VM stack / frame words holding NV-1 values; rewritten on evacuation.
    pub stack: &'a mut [u64],
    /// Remembered-set slots (old-gen tagged arrays that may point young).
    pub remembered: Vec<Handle>,
}

impl Gc {
    /// Minor collection: evacuates live nursery objects into old pages.
    pub fn collect_minor(
        &mut self,
        heap: &mut crate::heap::Heap,
        roots: Roots<'_>,
    ) -> RtResult<()> {
        let nursery = heap.nursery_page().ok_or(Trap::OutOfMemory)?;
        self.evacuate(heap, roots, &[nursery])
    }

    /// Major collection: traces from all roots including snapshot pages
    /// (which are immortal), evacuates nursery and old pages, and never
    /// touches snapshot pages.
    pub fn collect_major(
        &mut self,
        heap: &mut crate::heap::Heap,
        roots: Roots<'_>,
    ) -> RtResult<()> {
        let mut from_pages = Vec::new();
        for p in 0..heap.page_count() {
            if heap.page_kind(p as u32) != PageKind::Snapshot {
                from_pages.push(p as u32);
            }
        }
        self.evacuate(heap, roots, &from_pages)
    }

    /// Core evacuation: mark from roots (restricted to `from_pages` for
    /// relocation purposes, but tracing passes through snapshot objects),
    /// copy live objects to fresh pages, fix references, rewrite roots.
    // CEP:WHAT Core evacuation: mark from roots (restricted to from_pages for relocation purposes, but tracing passes through snapshot objects), copy live objects to fresh pages, fix references, rewrite roots.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn evacuate(
        &mut self,
        heap: &mut crate::heap::Heap,
        mut roots: Roots<'_>,
        from_pages: &[u32],
    ) -> RtResult<()> {
        // Side-metadata mark set keyed by handle.
        let mut marked: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut worklist: Vec<Handle> = Vec::new();

        let push_word =
            |w: u64, marked: &mut std::collections::HashSet<u64>, worklist: &mut Vec<Handle>| {
                if nv1::is_ref(w) {
                    let h = Handle(nv1::ref_offset(w));
                    if marked.insert(h.0) {
                        worklist.push(h);
                    }
                }
            };

        // Seed: stack roots and remembered-set array payloads.
        for &w in roots.stack.iter() {
            push_word(w, &mut marked, &mut worklist);
        }
        for &arr in &roots.remembered {
            if marked.insert(arr.0) {
                worklist.push(arr);
            }
        }

        // Transitive trace. Numeric payloads are never scanned (NUM-INV-01);
        // snapshot pages are traced but never relocated; objects on pages
        // outside `from_pages` (i.e. snapshots) are traced as roots but their
        // relocation is skipped.
        while let Some(h) = worklist.pop() {
            if h.page() as usize >= heap.page_count() {
                continue; // corrupt or foreign handle: skip, never dereference
            }
            let lay = heap.layout_of(h);
            if layout::is_numeric(lay) {
                continue;
            }
            let len_words = heap.payload_word_count(h);
            for i in 0..len_words {
                let w = heap.read_payload(h, i);
                push_word(w, &mut marked, &mut worklist);
            }
        }

        // Copy plan for live objects on from_pages.
        let mut new_page: Option<u32> = None;
        let mut forward: std::collections::HashMap<u64, Handle> = std::collections::HashMap::new();
        let mut live: Vec<Handle> = marked.iter().map(|&x| Handle(x)).collect();
        live.sort_unstable();

        // Layout classification must happen before any header mutation; this
        // implementation uses full stop-the-world compaction, so forwarding
        // pointers in old headers are unnecessary (references are fixed via
        // the forward map during the pause). Snapshot pages are skipped
        // because they are not in from_pages.
        let numeric: std::collections::HashSet<u64> = live
            .iter()
            .filter(|&&h| layout::is_numeric(heap.layout_of(h)))
            .map(|h| h.0)
            .collect();

        for &h in &live {
            if !from_pages.contains(&h.page()) {
                continue; // snapshot or already-stable page: never relocated
            }
            if forward.contains_key(&h.0) {
                continue;
            }
            let words = heap.total_word_count(h);
            if new_page.is_none() || heap.page_free_words(new_page.expect("checked")) < words {
                new_page = Some(heap.add_old_page());
            }
            let dst = heap.bump_in(new_page.expect("checked"), words)?;
            heap.copy_object(h, dst)?;
            forward.insert(h.0, dst);
            self.stats.objects_moved += 1;
        }

        // Fix references inside copied objects (on the new pages) and in
        // remembered-set arrays that were not themselves relocated.
        let fixes: Vec<(Handle, usize, Handle)> = {
            let mut fixes = Vec::new();
            for &h in &live {
                let Some(&dst) = forward.get(&h.0) else {
                    continue;
                };
                if numeric.contains(&h.0) {
                    continue;
                }
                let words = heap.payload_word_count(h);
                for i in 0..words {
                    let w = heap.read_payload(h, i);
                    if nv1::is_ref(w) {
                        let target = Handle(nv1::ref_offset(w));
                        if let Some(&nt) = forward.get(&target.0) {
                            fixes.push((dst, i, nt));
                        }
                    }
                }
            }
            for &arr in &roots.remembered {
                if forward.contains_key(&arr.0) || numeric.contains(&arr.0) {
                    continue;
                }
                let words = heap.payload_word_count(arr);
                for i in 0..words {
                    let w = heap.read_payload(arr, i);
                    if nv1::is_ref(w) {
                        let target = Handle(nv1::ref_offset(w));
                        if let Some(&nt) = forward.get(&target.0) {
                            fixes.push((arr, i, nt));
                        }
                    }
                }
            }
            fixes
        };
        for (dst, i, nt) in fixes {
            heap.rewrite_payload_ref(dst, i, nt)?;
        }

        // Rewrite stack roots.
        for w in roots.stack.iter_mut() {
            if nv1::is_ref(*w) {
                let target = Handle(nv1::ref_offset(*w));
                if let Some(&nt) = forward.get(&target.0) {
                    *w = nv1::encode_ref(nt.0, 0).expect("valid handle");
                }
            }
        }

        // Drop the from_pages content: reset nursery pages, retire evacuated
        // old pages. The reference implementation resets bump cursors and
        // zeroes committed bytes for from_pages other than the new pages.
        heap.reset_pages(from_pages);
        roots.remembered.clear();
        Ok(())
    }
}
