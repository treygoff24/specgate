# Hardening Phase — Implementation Plan v2

**Date:** 2026-03-08
**Author:** Lumen (Opus 4.6)
**Revised after:** Adversarial reviews by Athena (Gemini) and Opus red-team
**Branch:** `hardening/cli-refactor` (P1), `hardening/adversarial-zoo` (P3), `hardening/sarif-output` (P4)
**Prerequisite:** Phase 5 complete (478 tests, master @ `137dd86`)

---

## Changes from v1

1. **P1 visibility strategy made explicit** — every task specifies `pub(crate)` items and re-export rules
2. **P1 test migration mapped** — which tests move with which functions
3. **P1.12 split** — render trait is a design task, not a "pure move"
4. **P4 (SARIF) decoupled from P1.12** — ships against existing `verdict::format`, no trait needed
5. **P2 deferred** — policy governance moved to separate future plan (needs deeper design for CI shallow clones, YAML structural diffing, rename detection)
6. **P3 fixture assertions enriched** — structured expected-behavior per scenario
7. **P5-P9 removed from scope** — labeled as backlog requiring separate planning passes
8. **Checkpoint assertions added** — measurable gates at P1.6, P1.11, P1.12

---

## Execution Order (Revised)

```
P1 (CLI Refactor) ─── strictly sequential, 12 tasks ──→ done
P3 (Adversarial Zoo) ─── independent, parallel with P1 ──→ done  
P4 (SARIF Output) ─── independent, parallel with P1 ──→ done
```

No dependency chain between P1, P3, P4. All three can run in parallel.
P4 no longer needs the render trait — it extends existing `verdict::format`.

---

## Visibility & Re-Export Strategy (applies to all P1 tasks)

### The problem

`src/cli/mod.rs` contains ~50 private functions and ~15 private types. The `doctor` command depends on ~30 of these private items across its compare, trace, and overview flows. Moving functions to new files changes Rust's visibility rules.

### The solution

1. **Every extracted function/type gets `pub(crate)` visibility.** Not `pub` (too broad) or private (breaks cross-module access within `cli/`).

2. **`mod.rs` re-exports all submodules:**
   ```rust
   // src/cli/mod.rs (after refactor)
   pub mod check;
   mod analysis;
   mod baseline_cmd;
   mod blast;
   mod doctor;
   pub mod init;
   mod project;
   mod severity;
   mod types;
   mod util;
   pub mod validate;

   // Re-export for `use super::*` in submodules
   pub(crate) use analysis::*;
   pub(crate) use baseline_cmd::*;
   pub(crate) use blast::*;
   pub(crate) use project::*;
   pub(crate) use severity::*;
   pub(crate) use types::*;
   pub(crate) use util::*;
   ```

3. **Doctor submodules can import shared CLI helpers without widening public API surface.** The re-exports from `mod.rs` keep shared items available inside `cli/`, while `doctor/mod.rs` and sibling files can use explicit `super::` or `crate::cli::` imports as needed.

4. **Tests that call private functions move with those functions.** Specifically:
   - `escape_yaml_double_quoted_escapes_control_chars` (line 3559) → moves to `init.rs` test module
   - `parse_structured_snapshot_keeps_schema_version_validation` (line 3480) → moves to `doctor/tests.rs`
   - All other tests in `mod tests` call `run()` (the CLI entry point) and are end-to-end. They stay in `mod.rs` until P1.12, then move to a dedicated `tests.rs` integration module.

### Re-export invariant

After every P1 task: `use super::*` in any `src/cli/*.rs` file must resolve all previously-accessible items. If a task moves an item, the re-export must be added in the same commit.

---

## P1: CLI Refactor (12 tasks)

**Goal:** Split `src/cli/mod.rs` (3,645 lines) into focused submodules.
**Branch:** `hardening/cli-refactor` off `master`
**Gate:** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && ./scripts/ci/mvp_gate.sh`
**All tasks are strictly sequential. No parallel execution within P1.**

### P1.1 — Extract shared types (~200 LOC move)

**Move to `src/cli/types.rs`:**
- Exit code constants: `EXIT_CODE_PASS`, `EXIT_CODE_POLICY_VIOLATIONS`, `EXIT_CODE_RUNTIME_ERROR`, `EXIT_CODE_DOCTOR_MISMATCH`
- `CliRunResult` struct + all `impl` methods (`json`, `clap_error`)
- `LoadedProject` struct
- `AnalysisArtifacts` struct
- `GovernanceHashes` struct
- All output structs: `ErrorOutput`, `ValidateOutput`, `ValidateIssueOutput`, `BaselineOutput`, `InitOutput`

**Visibility changes:** All items become `pub(crate)`.

**Re-export in `mod.rs`:** `pub(crate) use types::*;`

**Tests affected:** None directly. All tests use `run()` or `EXIT_CODE_*` constants — the constants become accessible through the re-export.

**Commit:** `refactor(cli): extract shared types to types.rs`
**Verify:** `cargo test` passes. `mod.rs` decreases by ~200 lines.

---

### P1.2 — Extract utility functions (~130 LOC move)

**Move to `src/cli/util.rs`:**
- `runtime_error_json()`
- `resolve_against_root()`
- `record_timing()`
- `compute_governance_hashes()` (depends on `GovernanceHashes` from types.rs — accessible via `use super::*`)
- `compute_telemetry_summary()`
- `project_fingerprint()`
- `hash_canonical_json()`
- `canonicalize_json()`

**Visibility changes:** All items become `pub(crate)`.

**Re-export in `mod.rs`:** `pub(crate) use util::*;`

**Tests affected:** None. These functions are only called from handler functions.

**Commit:** `refactor(cli): extract utility functions to util.rs`
**Verify:** `cargo test` passes.

---

### P1.3 — Extract severity helpers (~100 LOC move)

**Move to `src/cli/severity.rs`:**
- `severity_rank()`
- `dependency_violation_severity()`
- `rule_ids_match()`
- `severity_for_constraint_rule()`
- `boundary_constraint_module()`
- `boundary_violation_severity()`

**Visibility changes:** All items become `pub(crate)`.

**Re-export in `mod.rs`:** `pub(crate) use severity::*;`

**Tests affected:** None. Called only from `analyze_project()`.

**Commit:** `refactor(cli): extract severity helpers to severity.rs`
**Verify:** `cargo test` passes.

---

### P1.4 — Extract `load_project()` (~30 LOC move)

**Move to `src/cli/project.rs`:**
- `load_project()` function

**Visibility changes:** `pub(crate)`.

**Re-export in `mod.rs`:** `pub(crate) use project::*;`

**Tests affected:** None. Called from handler functions.

**Commit:** `refactor(cli): extract load_project to project.rs`
**Verify:** `cargo test` passes.

---

### P1.5 — Extract `analyze_project()` (~200 LOC move)

**Move to `src/cli/analysis.rs`:**
- `analyze_project()` function

**Dependencies:** Uses types from `types.rs` (P1.1), severity helpers from `severity.rs` (P1.3), `load_project` is in `project.rs` (P1.4). All accessible via `use super::*`.

**Visibility changes:** `pub(crate)`.

**Re-export in `mod.rs`:** `pub(crate) use analysis::*;`

**Tests affected:** None. Called from handler functions.

**Commit:** `refactor(cli): extract analyze_project to analysis.rs`
**Verify:** `cargo test` passes.

---

### P1.6 — Extract blast radius helpers (~135 LOC move)

**Move to `src/cli/blast.rs`:**
- `BlastRadiusData` struct
- `build_blast_radius()`
- `derive_blast_edge_pairs()`
- `build_blast_radius_data()`

**Visibility changes:** All items become `pub(crate)`.

**Re-export in `mod.rs`:** `pub(crate) use blast::*;`

**Tests affected:** None. Called from `handle_check()`.

**Commit:** `refactor(cli): extract blast radius helpers to blast.rs`

**🔴 CHECKPOINT:** `mod.rs` must be under 2,900 lines. `blast.rs` must exist. `cargo test` and `cargo clippy` pass.

---

### P1.7 — Expand `validate.rs` (~60 LOC move)

**Move to existing `src/cli/validate.rs`:**
- `handle_validate()` function body

**Current `validate.rs` state:** 29 lines (just `ValidateArgs` struct). Expand to include the handler.

**Add at top of file:** `use super::*;`

**Visibility:** `handle_validate` becomes `pub(crate)`.

**`mod.rs` change:** `run()` calls `validate::handle_validate(args)` instead of `handle_validate(args)`.

**Tests affected:** None. E2E tests use `run()`.

**Commit:** `refactor(cli): move handle_validate into validate.rs`
**Verify:** `cargo test` passes.

---

### P1.8 — Expand `check.rs` (~600 LOC move)

**Move to existing `src/cli/check.rs`:**
- `handle_check()` function
- `handle_check_with_diff()` function (already `pub`)

**Current `check.rs` state:** 331 lines (args, types, diff mode). These functions join their arg definitions.

**Add/verify at top of file:** `use super::*;`

**Visibility:** Both become `pub(crate)`.

**`mod.rs` change:** `run()` calls `check::handle_check(args)`.

**Tests affected:** None. E2E tests use `run()`.

**Commit:** `refactor(cli): move check handlers into check.rs`
**Verify:** `cargo test` passes.

---

### P1.9 — Expand `init.rs` (~250 LOC move)

**Move to existing `src/cli/init.rs`:**
- `handle_init()`
- `InitScaffoldSpec` struct
- `INIT_COMMON_ROOT_MODULE_DIRS` constant
- `infer_init_scaffold_specs()`
- `infer_single_module_path()`
- `infer_root_module_path()`
- `write_scaffold_file()`
- `escape_yaml_double_quoted()`

**Current `init.rs` state:** 48 lines (just `InitArgs`). Expand to include all init logic.

**Add at top of file:** `use super::*;`

**Visibility:** All items become `pub(crate)`.

**Test migration:** Move `escape_yaml_double_quoted_escapes_control_chars` test (line 3559) AND `init_quotes_spec_dir_with_special_chars` test (line 3564) into a `#[cfg(test)] mod tests` block in `init.rs`. Both tests directly exercise init-specific functions.

**`mod.rs` change:** `run()` calls `init::handle_init(args)`.

**Commit:** `refactor(cli): move init handler and helpers into init.rs`
**Verify:** `cargo test` passes. Both migrated tests run.

---

### P1.10 — Extract baseline command (~125 LOC move)

**Move to `src/cli/baseline_cmd.rs` (new file):**
- `handle_baseline()` function

**Add at top:** `use super::*;`

**Visibility:** `pub(crate)`.

**Re-export in `mod.rs`:** `pub(crate) use baseline_cmd::*;`

**`mod.rs` change:** `run()` calls `baseline_cmd::handle_baseline(args)`.

**Tests affected:** None. E2E tests use `run()`.

**Commit:** `refactor(cli): extract baseline command to baseline_cmd.rs`
**Verify:** `cargo test` passes.

---

### P1.11 — Move remaining doctor logic into `src/cli/doctor/` (~1,100 LOC move)

**Move into `src/cli/doctor/` submodules rooted at `src/cli/doctor/mod.rs`:**

*Types and structs:*
- `DoctorCompareFocus`, `DoctorCompareFocusOutput`, `DoctorCompareResolutionOutput`
- `TraceSource` enum, `ParsedTraceData`, `TraceResolutionRecord`, `ParsedTraceResult`, `TraceResultKind`, `TraceParserKind`
- `STRUCTURED_TRACE_SCHEMA_VERSION` const
- All `DoctorOutput`, `DoctorOverlapOutput`, `DoctorCompareOutput` structs
- `serialize_trace_result_kind` helper

*Functions:*
- `build_doctor_compare_focus()`
- `filter_edges_for_focus()`
- `load_trace_source()`
- `is_command_available()`
- `structured_trace_schema_version()`
- `has_structured_snapshot_shape()`
- `parse_structured_trace_data()`
- `parse_legacy_trace_data()`
- `parse_trace_data()`
- `parsed_trace_data_from_structured_snapshot()`
- `structured_snapshot_from_parsed_trace()`
- `write_structured_snapshot()`
- `collect_trace_data_iterative()`
- `parse_tsc_trace_text_records()`
- `finalize_pending_resolution()`
- `json_string_field()`
- `infer_trace_result_kind()`
- All `parse_tsc_*_line` helpers
- `path_contains_node_modules()`
- `normalize_trace_path()`
- `doctor_resolution_from_specgate()`
- `derive_tsc_focus_resolution()`
- `parity_verdict_for_status()`
- `classify_doctor_compare_mismatch()`
- `classify_focus_mismatch_tag()`
- `matches_js_runtime_extension()`
- `resolution_path_looks_types()`
- `build_actionable_mismatch_hint()`
- `doctor_compare_beta_channel_enabled()`
- `build_workspace_packages_info()`

**Visibility:** Items stay private within the `doctor` module tree unless needed by other modules. `build_workspace_packages_info()` becomes `pub(crate)` (used from `handle_check`).

**Test migration:** Move `parse_structured_snapshot_keeps_schema_version_validation` test (line 3480) and `doctor_compare_auto_mode_scans_nested_json_trace_payload` test (line 3431) into `src/cli/doctor/tests.rs`. Both test doctor-specific functions.

**Note:** This step lands directly in a split `src/cli/doctor/` layout (`mod.rs`, `compare.rs`, `overview.rs`, `trace_*`, etc.) instead of first creating a new `doctor.rs` monolith. The important invariant is keeping doctor-only logic out of `mod.rs` while preserving behavior and deterministic output.

**`mod.rs` change:** No new re-exports needed (doctor items stay internal to doctor).

**Commit:** `refactor(cli): split doctor command into submodules`

**🔴 CHECKPOINT:** `mod.rs` must be under 500 lines. The `src/cli/doctor/` subtree owns the extracted doctor logic. All migrated tests pass. `cargo test` and `cargo clippy` clean.

---

### P1.12 — Final `mod.rs` trim + integration test module

**This is NOT a "pure move" — it involves restructuring.**

**What `mod.rs` should contain after P1.11:**
- Module declarations + re-exports (~30 lines)
- `Cli` struct, `Command` enum, arg types for top-level commands (~80 lines)
- `run()` function (~20 lines)
- `BaselineArgs`, `InitArgs`, `CommonProjectArgs` structs (~30 lines)
- Remaining E2E tests in `#[cfg(test)] mod tests` (~400 lines, 16 tests)

**Tasks:**
1. Move `BaselineArgs`, `InitArgs` (the clap structs) to their respective command modules (`baseline_cmd.rs`, `init.rs`). These are closely coupled to their handlers.
2. Move `DoctorArgs`, `DoctorCommand`, `DoctorCompareArgs`, `DoctorCompareParserMode` to `src/cli/doctor/mod.rs`. Currently in `mod.rs` lines 164-211.
3. Verify `mod.rs` is under 150 lines (module decls + Cli + Command + run + re-exports + CommonProjectArgs).
4. Move the E2E tests to `src/cli/tests.rs` (new file). These tests use `run()` which remains in `mod.rs` and is accessible via `use super::*`.

**DO NOT attempt the Renderer trait in this task.** The render trait is deferred to a future design spike (see note below). The existing `verdict::format` module continues to work as-is.

**Commit:** `refactor(cli): final mod.rs trim to ~100 lines`

**🔴 CHECKPOINT:** `mod.rs` must be under 150 lines. `tests.rs` must contain the migrated E2E tests. All 478+ tests pass. Full gate green.

### Note: Renderer Trait (Deferred)

Both reviewers flagged that the proposed `Renderer` trait is a redesign, not a move. The existing `verdict::format` module has functions with different signatures (`format_violation_human` takes `&Path + &FingerprintedViolation`, `format_verdict_human` takes `&Verdict`, `format_verdict_ndjson` takes `&Verdict`). The diff-mode path renders individual violations, not full verdicts.

**Decision:** Defer the render trait to a separate design spike after P1. SARIF (P4) ships without it by extending the existing `verdict::format` module directly.

---

## P3: Adversarial Fixture Zoo (4 tasks)

**Goal:** Test how agents actually fail. 14 adversarial scenarios (12 original + 2 from Athena's review).
**Branch:** `hardening/adversarial-zoo` off `master`
**Depends on:** Nothing. Fully parallel with P1 and P4.
**Gate:** `cargo test`

### Fixture assertion format

Each fixture directory contains:
```
tests/fixtures/adversarial/<scenario>/
├── specgate.config.yml
├── modules/*.spec.yml
├── src/**/*.ts
└── expected.yml          # structured assertion
```

`expected.yml` schema:
```yaml
assertion: catch | clean_pass | diagnostic
violations:                     # only when assertion=catch
  - rule: "rule.id"
    from_module: "module-a"
    to_module: "module-b"       # optional
gap_reason: null                # only when assertion=clean_pass
future_priority: null           # e.g. "P9" for import hygiene
notes: "Why this scenario matters"
```

### P3.1 — Fixture batch 1: Catchable scenarios (~500 LOC fixtures)

6 scenarios specgate SHOULD catch today:

| Scenario | Assertion | Expected rule |
|----------|-----------|---------------|
| `cross-layer-shortcut` | `catch` | `boundary.never_imports` or `enforce_layer` |
| `barrel-re-export-chain` | `catch` | boundary violation through re-export chain |
| `circular-via-re-export` | `catch` | `no_circular_dependencies` |
| `ownership-overlap` | `diagnostic` | doctor reports overlap |
| `orphan-module` | `diagnostic` | spec with zero matched files |
| `wildcard-re-export-leak` | `catch` | boundary violation via `export *` (from Athena) |

**Commit:** `test(adversarial): add catchable scenario fixtures`

---

### P3.2 — Fixture batch 2: Known gaps (~500 LOC fixtures)

8 scenarios specgate explicitly CANNOT catch (by design or not yet implemented):

| Scenario | Assertion | Gap reason | Future |
|----------|-----------|------------|--------|
| `deep-third-party-import` | `clean_pass` | No deep-import rules yet | P9 |
| `test-helper-leak` | `clean_pass` | No test boundary rules yet | P9 |
| `dynamic-import-evasion` | `clean_pass` | Static analysis only (by design) | Won't fix |
| `conditional-require` | `clean_pass` | Static analysis only (by design) | Won't fix |
| `type-import-downgrade` | `clean_pass` | No type vs value tracking | Future |
| `aliased-deep-import` | `clean_pass` or `catch` | Depends on resolver + tsconfig | Verify |
| `path-traversal` | `catch` or `clean_pass` | `../../../` escape (from Athena) | Verify |
| `hallucinated-import` | `clean_pass` | Unresolved import, currently silent | P6 |

For `aliased-deep-import` and `path-traversal`: build the fixture and actually run it to determine current behavior. The assertion is determined empirically, not assumed.

**Commit:** `test(adversarial): add known-gap scenario fixtures`

---

### P3.3 — Integration test runner (~250 LOC)

New file: `tests/adversarial_fixtures.rs`

For each fixture:
1. Read `expected.yml`
2. Run `specgate check --project-root <fixture> --no-baseline --format json`
3. Parse verdict JSON
4. Assert based on `expected.yml`:
   - `assertion: catch` → verify expected violation rule IDs present, correct modules
   - `assertion: clean_pass` → verify exit code 0, no violations
   - `assertion: diagnostic` → run `specgate doctor --project-root <fixture>`, verify relevant output

**Commit:** `test(adversarial): add integration test runner`

---

### P3.4 — Gap documentation (~200 LOC docs)

New doc: `docs/reference/adversarial-testing.md`

Catalog:
- What specgate catches and how (with rule IDs)
- What specgate cannot catch and why (design limits vs not-yet-implemented)
- For each "won't fix" gap: explain why static analysis fundamentally can't catch it
- For each "future" gap: reference the priority that would address it

Update CHANGELOG.

**Commit:** `docs: add adversarial testing catalog`

---

## P4: SARIF Output (3 tasks)

**Goal:** Native SARIF 2.1.0 output for GitHub Code Scanning.
**Branch:** `hardening/sarif-output` off `master`
**Depends on:** Nothing. Ships against existing `verdict::format` module.
**Gate:** `cargo test`

### P4.1 — SARIF formatter (~300 LOC)

New function in `src/verdict/format.rs`:
```rust
pub fn format_verdict_sarif(verdict: &Verdict) -> String
```

SARIF 2.1.0 structure:
- `$schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json"`
- `version: "2.1.0"`
- `runs[0].tool.driver.name: "specgate"`
- `runs[0].tool.driver.version: <tool_version>`
- `runs[0].tool.driver.rules[]`: one `reportingDescriptor` per unique rule ID
- `runs[0].results[]`: one `result` per violation
  - `ruleId`, `level` (error/warning → error/warning), `message.text`
  - `locations[0].physicalLocation.artifactLocation.uri` (repo-relative path)
  - `locations[0].physicalLocation.region.startLine` / `startColumn` (when available)
  - `fingerprints.specgate/v1` (reuse existing fingerprint for baseline stability)

**Tests:** 5+ unit tests:
- Valid SARIF JSON structure
- Rule descriptors populated
- Violation locations correct
- Fingerprints present
- Empty violations produces valid SARIF with zero results

**Commit:** `feat(verdict): add SARIF 2.1.0 output formatter`

---

### P4.2 — Wire into check command (~30 LOC)

- Add `Sarif` variant to `OutputFormat` enum in `check.rs`
- Wire `--format sarif` in `handle_check()` to call `format_verdict_sarif()`
- Also wire in `handle_check_with_diff()` for consistency (SARIF for diff mode renders the filtered violations as a verdict)

**Tests:** 2 integration tests:
- `specgate check --format sarif` produces valid JSON
- Violations appear in SARIF output

**Commit:** `feat(cli): wire --format sarif into check command`

---

### P4.3 — GitHub Actions example + docs (~100 LOC docs)

New doc: `docs/reference/sarif-github-actions.md`

Example workflow:
```yaml
- name: Run specgate
  run: specgate check --format sarif > specgate.sarif

- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: specgate.sarif
```

Update getting-started, changelog.

**Commit:** `docs: add SARIF GitHub Actions integration guide`

---

## Deferred Work

### P2: Policy Governance → Separate planning pass
Needs deeper design for:
- CI shallow clone handling (`fetch-depth: 0` requirement or `git archive` approach)
- YAML structural diffing that handles renames, deletions, parse failures
- Module rename detection (currently classified as delete + add)
- LOC estimate was 900; reviewers estimate 2,000+

### P5-P9: Incremental improvements → Backlog
Each needs its own planning pass with implementation-level detail:
- P5 (Ownership): overlap semantics, output format
- P6 (Edge Classification): where classification happens in pipeline
- P7 (Baseline v2): schema migration strategy
- P8 (Visibility Model): namespace hierarchy semantics
- P9 (Import Hygiene): resolver integration for deep-import detection

---

## Summary

| Priority | Name | Tasks | Est. LOC | Depends on | Risk |
|----------|------|-------|----------|------------|------|
| P1 | CLI Refactor | 12 | ~3,000 (moved) | — | Medium-High |
| P3 | Adversarial Zoo | 4 | ~1,450 | — | Low |
| P4 | SARIF Output | 3 | ~430 | — | Low |

**Total: 19 tasks, ~4,880 LOC (3,000 moved + 1,880 new)**

---

## Dispatch Strategy

All three priorities can run in parallel on separate branches.

**P1** (strictly sequential within):
- P1.1 → P1.2 → P1.3 → P1.4 → P1.5 → P1.6 [CHECKPOINT] → P1.7 → P1.8 → P1.9 → P1.10 → P1.11 [CHECKPOINT] → P1.12 [CHECKPOINT]
- Model: Vulcan high (precision needed for cross-reference updates)
- Strategy: One subagent per task, sequential. Parent verifies checkpoint assertions.

**P3** (partially parallel):
- P3.1 + P3.2 can parallelize (both create fixtures, different directories)
- P3.3 depends on P3.1 + P3.2
- P3.4 depends on P3.3
- Model: Spark for fixtures, Vulcan medium for test runner

**P4** (sequential, small):
- P4.1 → P4.2 → P4.3
- Model: Vulcan medium

**Checkpoint assertions:**
```
After P1.6:  mod.rs < 2,900 lines  |  blast.rs exists        |  cargo test passes
After P1.11: mod.rs < 500 lines    |  doctor/ subtree exists  |  cargo test passes
After P1.12: mod.rs < 150 lines    |  tests.rs exists         |  full gate green
```
