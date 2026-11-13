# VORTEX

## Abstract

VORTEX is a tiered execution engine specified in `design.md` and implemented in Rust. This repository contains (a) the architecture specification with every external review finding resolved, (b) a reference implementation of the execution stack - the NV-1 value model, the UGB bytecode contract, the runtime (heap, GC, AOSS, capabilities, dependency handshake, profiling, stencils), the T0 interpreter, the T1 stencil baseline, the T2/T3 optimizing pipeline, the T4 trace compiler, the tier controller with the CODE-1/IC-1/EXC-1/MEM-1/DEBUG-1 support subsystems, the unified graph IR with the VSDG overlay API and the FFI compatibility matrix - and (c) the mechanical standards enforcement tool required by `standard.md` chapter 50.

## Repository Layout

```text
design.md                  Architecture specification (review resolutions applied)
standard.md                CEP&CC 0.4 coding standard (mechanically enforced)
doc standard.md            Documentation style guide
.cep/cep_lint.json         Lint configuration (all 18 documented rules)
crates/vortex-nv1          NV-1 value model, corrected load barrier, PACKED_TAGGED model
crates/vortex-ugb          UGB opcodes, hybrid variable-length codec, assembler, verifier
crates/vortex-runtime      Heap, generational GC, AOSS, capability epochs, DependencyMap,
                           profile store, stencil pool
crates/vortex-t0           T0 interpreter (semantic reference for all tiers)
crates/vortex-t1           T1 stencil baseline: copy-and-patch (T1a/T1b), linear-scan
                           fallback (T1c), OSR trampolines, differential executor
crates/vortex-opt          T2/T3 optimizing pipeline: SSA graph, fold/GVN/LICM/ARS/
                           proven BCE/SROA, LLIR/MIR lowering, differential executor
crates/vortex-t4           T4 trace compiler: guarded recording, guard fusion,
                           deopt-to-T0 at the failing pc
crates/vortex-tier         Tier controller, CODE-1 code cache model, IC-1, EXC-1,
                           MEM-1, DEBUG-1
crates/vortex-ir           Unified graph, VSDG overlay API, GVN, SemanticCompatibilityMatrix
crates/vortex-cli          vortex binary: run / demo / disasm / tiers
tools/cep_lint             CEP&CC linter with self-test manifest (chapter 50)
```

## Implemented-Subsystem Map

| design.md section | Status in this repository |
| --- | --- |
| NV-1 (value model, INV-001..015) | Implemented and tested (`vortex-nv1`) |
| BARRIER-1 v1.2 (tag precedence) | Implemented: `needs_load_barrier`, heap load path; regression test |
| UGB-1 subset (encoding, capabilities, invariants 01-07) | Implemented: codec, verifier, epoch checks; tests |
| HEAP-1 revised (128-bit header, side metadata) | Implemented (header = layout handle + auxiliary word) |
| SNAP-1 v1.1 (non-relocatable snapshots, INV-08/09/10) | Implemented: page pinning, freeze check, store traps; tests |
| NUM-1 (numeric arrays, barrier elision) | Implemented: `Layout_NumericArray_F64/I64`, elision counters; tests |
| PROF-1 v1.1 (sampled back-edge counters, INV-07/08) | Implemented: `ProfileStore`; T4 request test |
| OSR-1 v1.1 section 6 (stencil purity, INV-06) | Modeled: `StencilPool`, shared OSR stub accounting; test |
| CONC-1 v1.1 section 2.1.1 (add-vs-invalidate handshake) | Implemented: seqlock + generation rejection; threaded test |
| REVISED 1 (overlay mutation API) | Implemented: transaction, event log, revision checks; tests |
| FFI-1 section 5 (SemanticCompatibilityMatrix, INV-FFI-007) | Implemented: pure checker; tests |
| SIMD-1 v1.1 (PACKED_TAGGED, B-F/B-I lane semantics) | Software model implemented; collision/overflow tests |
| T1 baseline (design.md sections 3-5, revised structure) | Implemented: copy-and-patch T1a/T1b over the pure stencil pool, T1c linear-scan fallback, OSR trampolines, DWARF stitching, shared unwind descriptor; differential tests |
| T2/T3 optimizing (design.md section 8) | Implemented: SSA graph (dominators, phi, rename), moderate and aggressive pass budgets, LLIR/MIR with explicit conversions, linear-scan allocation; differential + evidence tests |
| T4 trace compiler (design.md T4) | Implemented: guarded recording through the interpreter hook, guard fusion through GVN, deopt-to-T0 at the failing pc; differential tests |
| T5 whole-program optimizer | Specified only (the pipeline pieces it composes are implemented) |
| OSR-1 (entry contract, purity, instability) | Implemented: Interp::resume entry, frame-based plan entry, OSR-unstable bookkeeping; tests |
| CODE-1 (code cache) | Modeled: arenas, TLABs, blob lifecycle, sweeper; tests |
| IC-1 (inline caches) | Implemented: bounded state machine with megamorphic latch; tests |
| EXC-1 (exception tables) | Modeled: sorted tables, binary-search dispatch, coverage proof, deopt transfer; tests |
| MEM-1 (memory orders) | Implemented: reorder legality, fence elimination, target lowering tables; tests |
| DEBUG-1 (tooling) | Modeled: location-list coverage proof, perf-map publication; tests |

The tiering design in `design.md` is a specification for implementations; the reference engine executes UGB programs on T0 and exposes the runtime counters the higher tiers consume.

## Build and Test

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -p vortex-cli -- demo
cargo run -p vortex-cli -- run countdown
cargo run -p vortex-cli -- disasm countdown
cargo run -p vortex-cli -- tiers countdown
```

## Standards Enforcement

```sh
./target/debug/cep_lint --self-test   # 36 scenarios, one violating + one compliant per rule
./target/debug/cep_lint               # scans crates/ and tools/ under .cep/cep_lint.json
```

The configuration covers all 18 rules documented in `standard.md` chapter 50. Rules whose check primitives are C++-specific carry the `dialects` parameter per the 0.3 applicability note; `CEP-LINT-DOC-SYNC` enforces configuration-versus-document agreement on every run. The lint engine's own rule-table source is declared through `exempt_paths` because it must contain the marker literals it checks for.

Clippy runs with the workspace lint set (all warnings, `float_cmp`); `unsafe` code is forbidden workspace-wide (`unsafe_code = "forbid"`).

## Review Resolutions

`design.md` ends with a resolutions register mapping each review finding (BARRIER-1, SNAP-1, PROF-1, UGB-1, T1-OSR, SIMD-1, SIMD-RETCON, VSDG-OVERLAY, CONC-1, FFI-1, NUM-1) to its resolution section and status. Each resolution that is implementable at the foundation layer has an executable proof in the test suites listed above.

## Known Limitations

- The optimizing pipeline (T2/T3) compiles a documented subset of UGB; methods using `CALL`, the exception opcodes, or the capability opcodes reject compilation with typed errors and stay on T1/T0 (a legal, visible outcome - `vortex tiers` prints it).
- Boxed integers outside the i48 immediate domain are allocated as `Layout_BoxedInt` objects; re-tagging such values into a numeric array clamps to null in the reference reverse transition.
- `DependencyMap` guards list storage with a short lock instead of RCU pointer surgery (the workspace forbids `unsafe`); the observable protocol - sequence lock, retry, generation rejection - is the specified contract and is exercised under threads.
- The snapshot model is in-process (page freezing and immutability), not a serialized `.snap` file format.
