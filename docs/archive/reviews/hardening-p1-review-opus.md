# P1 CLI Refactor Review — Opus

**Branch:** `hardening/cli-refactor`  
**Commits:** `937bbc9..40f8020` (12 commits)  
**Reviewer:** opus-sub  
**Date:** 2026-03-08  
**Verdict:** ✅ Ship — no critical or high issues. Refactor is mechanically correct, all 289 tests pass, zero compiler warnings.

---

## Summary

The refactor splits `src/cli/mod.rs` from 3,645 lines into 13 focused modules. The delta is `+3,655 / -3,568` (net +87 lines from module boilerplate). No behavioral changes were detected — the diff is a pure move-and-reexport refactor.

---

## Findings

### 1. Visibility: `handle_check` and `handle_check_with_diff` are wider than necessary

**Severity:** Medium

In `check.rs`:
- `handle_check` is `pub(crate)` but only called from `mod.rs` (same module). `pub(super)` suffices.
- `handle_check_with_diff` is `pub` — fully public in the library crate. Before the refactor it was also `pub`, so this is pre-existing, but it exposes an internal dispatch function to library consumers.

Similarly, `handle_baseline`, `handle_validate`, and `handle_init` are all `pub(crate)` where `pub(super)` would be the minimum required.

`handle_doctor` is correctly `pub(super)`.

**Recommendation:** Align all command handlers to `pub(super)`. Consider demoting `handle_check_with_diff` to `pub(super)` as well unless there's an intended public API use case.

---

### 2. Visibility: `CliRunResult::json` and `CliRunResult::clap_error` widened from private to `pub`

**Severity:** Medium

Before the refactor, `json()` and `clap_error()` were inherent `fn` methods (no visibility keyword — private to the module). Now in `types.rs` they are `pub fn`, and since `types.rs` is `pub mod types` with `pub use types::*`, these methods are fully public to library consumers.

The original intent was clearly internal-only. Making them public expands the API surface unnecessarily.

**Recommendation:** Change to `pub(crate) fn` or `pub(super) fn`. They only need to be visible within the `cli` module tree.

---

### 3. Visibility: `doctor` module widened from `mod` to `pub(crate) mod`

**Severity:** Low

The doctor module was `mod doctor` (private to `cli`) before the refactor. It's now `pub(crate) mod doctor`. Combined with `pub(crate) use doctor::*`, this double-exposes every `pub(crate)` item in doctor: once through the glob re-export on `cli`, and once through the module path `cli::doctor::*`.

No code outside `cli` currently uses `cli::doctor::*` directly, so this is cosmetic, but it's an unnecessary broadening.

**Recommendation:** Revert to `mod doctor` (private). The glob re-export `pub(crate) use doctor::*` already makes the items available at `cli::*` scope.

---

### 4. Transit imports in `mod.rs` — structural debt

**Severity:** Medium

`mod.rs` carries ~19 `use` import lines from `crate::*` that are **not used by `mod.rs` itself** — they exist solely so submodules can access them via `use super::*`. The only items `mod.rs` actually needs are:

- `std::ffi::OsString`, `std::path::PathBuf`
- `clap::{Args, Parser, Subcommand}`
- Types from the re-exported submodules (already available via `pub(crate) use <mod>::*`)

Everything else (`BTreeMap`, `BTreeSet`, `Instant`, `fs`, `Path`, `serde::{Serialize, Deserialize}`, all `crate::baseline::*`, `crate::build_info`, `crate::deterministic::*`, `crate::graph::*`, `crate::resolver::*`, `crate::rules::*`, `crate::spec::*`, `crate::verdict::*`) is transit baggage.

This makes `mod.rs` misleading — it looks like it uses all these items, but it's just a pass-through. The `use super::*` pattern in submodules is the mechanism that makes this work, but it means:

1. Adding/removing a dependency in any submodule requires editing `mod.rs` imports.
2. It's impossible to tell from `mod.rs` which items are used where.
3. All submodules get access to **all** transit imports, even ones they don't need (e.g., `analysis.rs` gets `ReleaseChannel` and `StaleBaselinePolicy` that only `check.rs` and `doctor.rs` need).

**Recommendation (not for this PR):** Each submodule should import what it needs directly from `crate::*`. Remove transit imports from `mod.rs`. The `use super::*` pattern would then only pull in sibling submodule re-exports and locally-defined items. This is a follow-up refactor — not blocking for P1.

---

### 5. Redundant imports in submodules

**Severity:** Nitpick

Several submodules do `use super::*` (which brings in all `mod.rs` imports) and then redundantly re-import the same items:

- `severity.rs`: `use std::collections::BTreeMap; use std::path::Path;` — already in `super::*`
- `severity.rs`: `use crate::spec::{Severity, SpecConfig, SpecFile};` — `Severity` and `SpecConfig` already in `super::*`
- `blast.rs`: `use crate::deterministic::normalize_repo_relative;` — already in `super::*`
- `blast.rs`: `use crate::graph::DependencyGraph; use crate::resolver::{ModuleResolver, ModuleResolverOptions};` — all in `super::*`
- `doctor.rs` line 1: `use crate::deterministic::normalize_repo_relative;` — already in `super::*`
- `check.rs`: `use clap::Args;` — already in `super::*`

These are harmless (Rust allows shadowing use-imports) and won't cause compilation errors, but they add noise.

**Recommendation:** Either lean fully into `use super::*` and remove redundant imports, or remove `use super::*` and import explicitly. Don't do both.

---

### 6. Dead type: `ValidateArgs`

**Severity:** Low

`validate.rs` defines `pub struct ValidateArgs` with a `project_root: PathBuf` field. It's re-exported as `pub use validate::ValidateArgs` in `mod.rs`. However:

- `handle_validate` takes `CommonProjectArgs`, not `ValidateArgs`.
- `Command::Validate` uses `CommonProjectArgs`.
- `ValidateArgs` is never used anywhere in the codebase.

This is pre-existing (not introduced by the refactor) but worth noting since it's dead public API surface.

**Recommendation:** Remove `ValidateArgs` and its re-export, or wire it into the actual command handler.

---

### 7. Dead re-export: `InitArgsEnhanced`

**Severity:** Low

`pub use init::InitArgs as InitArgsEnhanced` re-exports `InitArgs` under an alias. Before the refactor, the old `init.rs` had a **different** `InitArgs` with `pub` fields (for library consumers to construct programmatically), while `mod.rs` had its own private `InitArgs` for CLI dispatch. The alias existed to distinguish them.

After the refactor, there's only one `InitArgs` (in `init.rs`) with **private** fields and `#[command(flatten)] common: CommonProjectArgs`. The alias is no longer meaningful — and the struct can't be constructed outside the module anyway because its fields are private.

`InitArgsEnhanced` is never referenced anywhere outside `mod.rs`.

**Recommendation:** Remove the `InitArgsEnhanced` alias. If the intent is to expose `InitArgs` publicly, re-export it as `pub use init::InitArgs`.

---

### 8. Duplicated test helpers across `tests.rs` and `doctor.rs`

**Severity:** Low

Both `tests.rs` and `doctor.rs` (in their `#[cfg(test)]` blocks) define identical helper functions:
- `write_file(root, relative_path, content)`
- `write_basic_project(root)`
- `write_basic_project_with_edge(root)`

**Recommendation:** Extract a shared `#[cfg(test)]` test utilities module (e.g., `test_support.rs` with `#[cfg(test)] mod test_support;`) or at minimum add a comment acknowledging the duplication. Not urgent — test code duplication is a minor maintenance cost.

---

### 9. `doctor.rs` is disproportionately large (1,660 lines)

**Severity:** Nitpick

The refactor successfully broke up `mod.rs`, but `doctor.rs` at 1,660 lines is now the new monolith. It contains:
- CLI arg types (DoctorArgs, DoctorCompareArgs, etc.)
- Output serialization types (DoctorOutput, DoctorCompareOutput, etc.)
- Trace parsing logic (structured snapshots, legacy tsc, JSON traversal)
- Edge comparison and mismatch classification
- Two command handlers (overview + compare)

This is a candidate for a second-pass split: `doctor/mod.rs`, `doctor/trace.rs`, `doctor/compare.rs`.

**Recommendation:** Future work — not blocking.

---

### 10. No re-export collision risk (confirmed)

**Severity:** N/A (positive finding)

All glob re-exports were checked for name collisions. No conflicts exist:
- `types.rs` exports: `CliRunResult`, `EXIT_CODE_*`, `LoadedProject`, `AnalysisArtifacts`, `GovernanceHashes`, `ErrorOutput`, `ValidateOutput`, `ValidateIssueOutput`, `BaselineOutput`, `InitOutput`
- `doctor.rs` exports: all prefixed with `Doctor*` or `Trace*`
- `util.rs` exports: `HashedSpec`, `runtime_error_json`, `resolve_against_root`, etc.
- No name overlaps between any re-exported modules.

---

### 11. Module boundaries are cohesive

**Severity:** N/A (positive finding)

Each file contains a well-scoped set of functions:
- `types.rs`: CLI result types and output serialization structs
- `util.rs`: JSON hashing, path resolution, telemetry summary
- `severity.rs`: Violation severity logic and workspace package info
- `project.rs`: Project loading (config + specs + validation)
- `analysis.rs`: Full policy analysis pipeline
- `blast.rs`: Git blast-radius computation
- `check.rs`: Check command args, output formats, handlers
- `validate.rs`: Validate command
- `init.rs`: Init command, scaffold generation, YAML escaping
- `baseline_cmd.rs`: Baseline generation command
- `doctor.rs`: Doctor command (overview + compare)
- `tests.rs`: E2E integration tests

One debatable placement: `build_workspace_packages_info` is in `severity.rs`. It's not severity-related — it's workspace discovery. But it was co-located with the severity functions in the original monolith and only used by `check.rs` and `doctor.rs`, so moving it to `util.rs` or its own file would be a reasonable future improvement.

---

### 12. No behavioral changes detected

**Severity:** N/A (positive finding)

Verified:
- All 289 tests pass with zero failures.
- `cargo check` with `-Wunused` produces zero warnings.
- The git diff is exclusively struct/function moves and visibility adjustments.
- No logic branches were added, removed, or reordered.
- All command dispatch in `run()` is unchanged.
- The `Command` enum variants map to the same handlers with the same argument types.

---

## Action Items

| # | Severity | Item | Effort |
|---|----------|------|--------|
| 1 | Medium | Tighten handler visibility to `pub(super)` | 5 min |
| 2 | Medium | Tighten `CliRunResult::json`/`clap_error` to `pub(crate)` | 2 min |
| 3 | Low | Revert `doctor` module to `mod doctor` | 1 min |
| 4 | Medium | (Follow-up) Move transit imports into submodules | 30 min |
| 5 | Nitpick | Clean up redundant imports in submodules | 10 min |
| 6 | Low | Remove dead `ValidateArgs` type | 2 min |
| 7 | Low | Remove dead `InitArgsEnhanced` alias | 1 min |
| 8 | Low | Extract shared test helpers | 15 min |
| 9 | Nitpick | (Future) Split `doctor.rs` further | 1 hr |

Items 1–3 are quick wins recommended before merge. Item 4 is recommended as a fast-follow. Items 5–9 are optional cleanup.
