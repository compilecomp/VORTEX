<!-- GENERATED FILE
Generator: scripts/gen_rosetta.py
Generator CEP:STATUS: complete
Generator CEP:EVIDENCE: cep_lint CEP-LINT-DOC-FILE registry check
Do not edit manually.
-->

# func_block_fires_on_undocumented_body (CS-a14)

Registered per the Rosetta Protocol (CEP&CC 68.2). Source: `tools/cep_lint/tests/engine.rs`.

## Tier 1: Beginner

Implements `func_block_fires_on_undocumented_body`.

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
