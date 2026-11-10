# VORTEX

## Abstract

VORTEX is a tiered execution engine specified in `design.md` and implemented in Rust. This repository contains (a) the architecture specification with every external review finding resolved, (b) a reference implementation of the foundation subsystems - the NV-1 value model, the UGB bytecode contract, the runtime (heap, GC, AOSS, capabilities, dependency handshake, profiling, stencils), the T0 interpreter, the unified graph IR with the VSDG overlay API and the FFI compatibility matrix - and (c) the mechanical standards enforcement tool required by `standard.md` chapter 50.

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
crates/vortex-ir           Unified graph, VSDG overlay API, GVN, SemanticCompatibilityMatrix
crates/vortex-cli          vortex binary: run / demo / disasm
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
| T1/T2/T3/T4/T5 compilers | Specified only (design.md); the T0 interpreter is the execution reference |
| EXC-1, IC-1, MEM-1, CODE-1, DEBUG-1 | Specified only |

The tiering design in `design.md` is a specification for implementations; the reference engine executes UGB programs on T0 and exposes the runtime counters the higher tiers consume.

## Build and Test

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -p vortex-cli -- demo
cargo run -p vortex-cli -- run countdown
cargo run -p vortex-cli -- disasm countdown
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

- The T0 interpreter does not implement `CALL` (no method-table linking); cross-method calls are a frontend concern in the tiered design.
- Boxed integers outside the i48 immediate domain are allocated as `Layout_BoxedInt` objects; re-tagging such values into a numeric array clamps to null in the reference reverse transition.
- `DependencyMap` guards list storage with a short lock instead of RCU pointer surgery (the workspace forbids `unsafe`); the observable protocol - sequence lock, retry, generation rejection - is the specified contract and is exercised under threads.
- The snapshot model is in-process (page freezing and immutability), not a serialized `.snap` file format.
