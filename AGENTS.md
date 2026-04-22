# AGENTS.md

This repo is designed for modular, compile-time extension of `stardive-api`.

## Goal
Add new API capabilities as isolated modules with explicit contracts and tests.

## Required steps for a new module
1. Create module files under `crates/stardive-api/src/modules/<module_name>.rs`.
2. Add request/response types to `crates/stardive-core/src/types.rs` when exposed publicly.
3. Register the module in `crates/stardive-api/src/modules/mod.rs` by adding a `ModuleDef` entry.
4. Add config toggle support in `crates/stardive-api/src/config.rs` (`STARDIVE_ENABLE_<MODULE>`).
5. Add unit tests for validation/behavior and integration tests for HTTP routes.
6. Update `README.md` with endpoint docs and examples.

## Module contract
- Must provide route registration via `register(router)`.
- Must provide capability health reporting via `capability(state)`.
- Must respect module enable/disable toggles.
- Must return structured JSON errors using shared error envelopes.

## Acceptance checklist
- `cargo test` passes in workspace.
- New routes appear under `/v1`.
- Health endpoint reports module capability status.
- CLI compatibility considered (`stardive-core` types updated when needed).
