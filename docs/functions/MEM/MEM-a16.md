<!-- GENERATED FILE
Generator: scripts/gen_rosetta.py
Generator CEP:STATUS: complete
Generator CEP:EVIDENCE: cep_lint CEP-LINT-DOC-FILE registry check
Do not edit manually.
-->

# evacuate_young_finds_old_roots_via_cards (MEM-a16)

Registered per the Rosetta Protocol (CEP&CC 68.2). Source: `crates/vortex-runtime/tests/concurrent_gc.rs`.

## Tier 1: Beginner

Implements `evacuate_young_finds_old_roots_via_cards`.

Why it exists: Documented in the function's CEP block.

If it fails: none

## Tier 2: Silicon Reality

Cost: documented in the CEP block

Assumptions: none

Evidence: cargo test

## Silent Failure Checklist

- Fallbacks: none; the function declares no failure points.
- Truncation: none documented; integer narrowing is governed by the CEP:ASSUMES field.
- Wrapping: no wrapping arithmetic documented; overflow behavior follows the CEP:ASSUMES field.
- Error codes: none; the function declares no failure points.
