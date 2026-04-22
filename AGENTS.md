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

## Release workflow (confirmation-gated)

Use this only after the user explicitly approves publishing.

### Semver bump rules
- Patch (`x.y.Z`) for bug fixes, refactors without public API break.
- Minor (`x.Y.z`) for new backward-compatible features.
- Major (`X.y.z`) for breaking public API/CLI behavior.

### Single release command
- Script: `scripts/release-crates.sh`
- Usage: `./scripts/release-crates.sh <x.y.z>`
- The script requires typing an exact confirmation phrase:
  - `CONFIRM RELEASE v<x.y.z>`

### What the script does (all-in-one)
1. Verifies clean git worktree and tag non-existence.
2. Bumps version in workspace `Cargo.toml`.
3. Syncs `stardive-core` dependency versions in crate manifests.
4. Runs `cargo test --workspace --all-targets`.
5. Runs `cargo publish --dry-run --workspace --exclude stardive-api`.
6. Creates release commit `chore(release): v<x.y.z>` and annotated tag `v<x.y.z>`.
7. Publishes crates in order:
   - `stardive-core`
   - `stardive` (with retry loop for index propagation)
8. Pushes `main` and the tag to GitHub.

### Agent safety rule
- Never run the release script (or manual publish/tag sequence) unless the user gives explicit release confirmation in the current thread.
