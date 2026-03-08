# Hardening Implementation Plan — Red-Team Review

**Reviewer:** Lumen (Opus 4.6), red-team role
**Date:** 2026-03-08
**Verdict:** ⛔ DO NOT approve for autonomous execution as-is. Requires revisions to P1 before launching.

---

## Executive Summary

The plan is well-structured and the overall decomposition is sound. However, it makes a critical false assumption that P1 tasks are "pure moves with zero behavior change." They are not. The existing `mod.rs` uses Rust's module privacy model in ways that will break on extraction. The plan also underestimates the coordination risk of 38 sequential-ish tasks and leaves P5–P9 as stubs that shouldn't be counted toward scope.

I'd approve P2, P3, and P4 for autonomous execution today. P1 needs a revision pass before any agent touches it.

---

## Finding 1: The "Pure Move" Assumption is Wrong

**Severity: BLOCKING for P1**

The plan labels P1.1–P1.6 as "independent extractions (pure moves)" and states "All existing tests pass unchanged. Zero behavior change." This is incorrect because of Rust's module visibility rules.

### 1a. Private functions called from `mod tests {}` via `use super::*`

The `#[cfg(test)] mod tests` block at line 2948 of `mod.rs` uses `use super::*;` and directly calls:

- `escape_yaml_double_quoted()` (line 1251, private `fn`) — tested at line 3553
- `parse_structured_trace_data()` (line 1732, private `fn`) — tested at line 3483
- `STRUCTURED_TRACE_SCHEMA_VERSION` (line 1609, private `const`) — asserted at line 3426

Under the plan:
- `escape_yaml_double_quoted` moves to `init.rs` (P1.9)
- `parse_structured_trace_data` moves to `doctor.rs` (P1.11)
- `STRUCTURED_TRACE_SCHEMA_VERSION` moves to `doctor.rs` (P1.11)

After those moves, `use super::*` in `mod tests` will no longer see these items unless they're re-exported or made `pub(crate)`. The tests will fail to compile.

**Fix required:** The plan must explicitly account for which test cases need to move with their target function, or which functions need visibility bumps. Concretely:
- `test escape_yaml_double_quoted_escapes_control_chars` (line 3553) should move to `init.rs`'s test module.
- `test parse_structured_snapshot_keeps_schema_version_validation` (line 3483) and the `doctor_compare_*` tests should move to `doctor.rs`'s test module.
- Alternatively, make these items `pub(crate)` and keep the tests centralized — but that loosens visibility, which the plan claims to avoid.

### 1b. `doctor.rs` already uses `use super::*`

The existing `src/cli/doctor.rs` (line 3) does `use super::*;`, meaning it relies on every private type and function in `mod.rs` being accessible via the parent module. This includes:

- `LoadedProject` (line 216, private struct)
- `AnalysisArtifacts` (line 224, private struct)
- All doctor output structs (`DoctorOutput`, `DoctorOverlapOutput`, `DoctorCompareOutput`, `DoctorCompareResolutionOutput`, `DoctorCompareFocusOutput`) — all private
- `DoctorArgs`, `DoctorCommand`, `DoctorCompareArgs`, `DoctorCompareParserMode` — all private
- `DoctorCompareFocus` (line 1388, private struct)
- `TraceSource`, `ParsedTraceData`, `TraceResultKind`, `TraceResolutionRecord`, `ParsedTraceResult`, `TraceParserKind` — all private
- Helper functions: `load_project`, `analyze_project`, `runtime_error_json`, `resolve_against_root`, `normalize_repo_relative` (re-exported from crate), `build_doctor_compare_focus`, `filter_edges_for_focus`, `load_trace_source`, `parse_trace_data`, `doctor_compare_beta_channel_enabled`, `write_structured_snapshot`, `derive_tsc_focus_resolution`, `parity_verdict_for_status`, `classify_doctor_compare_mismatch`, `build_actionable_mismatch_hint`, `build_workspace_packages_info`

When P1.1 moves `LoadedProject` and `AnalysisArtifacts` to `types.rs`, and P1.11 moves the doctor functions to `doctor.rs`, there's a chicken-and-egg problem: `doctor.rs` currently gets these types from `super::*`, but after P1.1, they'd be in `types.rs`. `doctor.rs`'s `use super::*` will still work IF `mod.rs` does `pub use types::*;` (as the plan states) — but only because `*` re-exports are transitive within the crate.

However, when P1.11 moves 30+ functions INTO `doctor.rs`, those functions currently call other private functions that are being moved to DIFFERENT files by P1.2–P1.6. For example, `handle_doctor_overview` (currently in `doctor.rs`) calls `analyze_project` (moving to `analysis.rs` in P1.5) and `runtime_error_json` (moving to `util.rs` in P1.2). After the move, `doctor.rs` would need `use super::analysis::analyze_project` or `mod.rs` must re-export from `analysis`.

The plan says `mod.rs` will do `pub use types::*;` but doesn't specify the re-export strategy for `util`, `severity`, `project`, `analysis`, or `blast`. If `mod.rs` doesn't re-export these, `doctor.rs`'s existing `use super::*` pattern breaks.

**Fix required:** The plan must specify the re-export strategy for every new submodule. The cleanest approach: `mod.rs` does `pub(crate) use {types, util, severity, project, analysis, blast}::*;` so that `use super::*` continues to work. This must be stated explicitly per task, not assumed.

### 1c. `handle_check_with_diff` is `pub fn`

`handle_check_with_diff` (line 848) is `pub fn`, meaning it's part of the module's public API. It's used... let me check... only from `handle_check` in the same module. If it moves to `check.rs`, it needs to remain callable from wherever it's currently called. The plan says P1.8 moves it to `check.rs`, and `handle_check` also moves there, so this is fine — but it's worth noting that its `pub` visibility means external code (tests, integration tests) could depend on it.

---

## Finding 2: P1.12's Render Trait Isn't a Pure Move — It's a Redesign

**Severity: HIGH — scope underestimate, potential breakage**

The plan says P1.12 will:
> Move existing human/json/ndjson formatting from `verdict::format` into `render/human.rs`, `render/json.rs`, `render/ndjson.rs` as trait impls

This is NOT a file move. It's a redesign. Here's why:

### 2a. `verdict::format` functions don't implement a trait today

`verdict::format` (760 lines) contains free functions:
- `format_violation_human()` — takes `&Path, &FingerprintedViolation`
- `format_violation_diff()` — takes `&Path, &FingerprintedViolation`
- `format_summary_table()` — takes `&Path, &[FingerprintedViolation]`
- `format_verdict_human()` — takes `&Verdict`
- `format_verdict_ndjson()` — takes `&Verdict`
- `ViolationStats` struct with `from_violations()` and `format_human()`

The proposed trait is:
```rust
pub trait Renderer {
    fn render_verdict(&self, verdict: &Verdict) -> String;
}
```

This is a lossy abstraction. The existing functions have different signatures — `format_violation_human` takes a `&Path` and a `&FingerprintedViolation`, not a `&Verdict`. `format_violation_diff` does too. `ViolationStats` is a standalone computation. These don't map cleanly to a single `render_verdict(&self, &Verdict)` method.

The `handle_check_with_diff` function (line 964) calls `format_violation_diff` per-violation and `ViolationStats::from_violations` separately. It doesn't render a `Verdict` — it renders individual `FingerprintedViolation` entries. The diff path would need either:
- A different trait method (`render_violation`, `render_stats`), or
- Conversion of the diff pipeline to build a full `Verdict` first (behavior change)

### 2b. `verdict::format` has 15 tests that import verdict types

The `#[cfg(test)]` block in `format.rs` (starting around line 297) has 15 tests that directly use `FingerprintedViolation`, `ViolationDisposition`, `PolicyViolation`, `Verdict`, `VerdictViolation`, `VerdictStatus`, `VerdictSummary`, `AnonymizedTelemetrySummary`, and `VERDICT_SCHEMA_VERSION` — all from `crate::verdict::*`.

If this code moves from `verdict::format` to `cli::render/*`, these tests lose their `super::*` access to verdict types and would need explicit `use crate::verdict::*;`. Not hard, but it's not zero work, and the plan claims "All existing tests pass unchanged."

### 2c. The module boundary is wrong

`verdict::format` is in the `verdict` crate module. Moving it to `cli::render` changes the conceptual ownership: rendering is now CLI-owned rather than verdict-owned. This is arguably correct (the CLI decides how to render), but it means `crate::verdict::format` ceases to exist. Any code that does `verdict::format::*` needs updating. Currently that's only `src/cli/mod.rs` (4 call sites at lines 691, 697, 964, 971), so the blast radius is small — but the plan should acknowledge this is a module deletion + recreation, not a move.

**Fix required:** P1.12 should be flagged as "new design work" with its own design spike, not lumped in with the "pure moves" of P1.1–P1.11. The trait interface needs to be designed before implementation. Consider whether `Renderer` should have multiple methods or just `render_verdict`. Consider whether the diff-mode rendering path also goes through the trait or remains standalone.

---

## Finding 3: P2's Git Spec Parsing is O(n) Git Processes

**Severity: MEDIUM — performance concern, not a blocker**

P2.3 proposes loading spec files from a git ref via `git show <ref>:<path>`. For a project with 50 spec files, that's 50 `git show` invocations. Each one spawns a process, reads from the git object database, and parses YAML.

### Better approaches:

1. **`git diff --name-only <ref> -- '*.spec.yml'`** — First check which spec files actually changed. If 3 of 50 changed, you only need 3 `git show` calls for the old versions, plus you already have the current versions on disk. This is almost always the right optimization.

2. **`git archive <ref> -- <spec_dir>/`** — Single invocation that exports all spec files from the ref as a tar archive. Parse the tar in-memory. Saves (n-1) process spawns.

3. **`git show <ref>:<dir>/` with a tree listing** — Use `git ls-tree <ref> <spec_dir>/` to list files, then batch `git show` calls (or use `git cat-file --batch`).

Option 1 is the lowest-effort, highest-impact fix. The plan should require it.

**Fix required:** P2.3 should use `git diff --name-only` to scope the work, then `git show` only for changed specs. Add this to the task description.

---

## Finding 4: P3 Adversarial Fixtures Need Expected Behavior Specs, Not Just "Don't Crash"

**Severity: MEDIUM — spec gap**

The plan says for scenarios specgate can't catch: "document as `expected_gap.md` and assert specgate at least doesn't crash."

This is insufficient. For each adversarial scenario, the test should assert one of:

1. **Specgate catches it** → assert specific violation rule ID and message
2. **Specgate reports a diagnostic** → assert a warning or info-level output
3. **Specgate explicitly cannot catch it** → assert clean exit (no crash), document WHY in the fixture's `expected_gap.md`, and specify whether a future capability should address it

For the specific scenarios:

| Scenario | Can specgate catch it? | Expected behavior |
|----------|----------------------|-------------------|
| `cross-layer-shortcut` | ✅ Yes — `never_imports` or layer rules | Assert specific violation |
| `deep-third-party-import` | ❌ Not today — no deep-import rules yet | Assert clean pass + document for P9 |
| `test-helper-leak` | ❌ Not today — no test boundary rules yet | Assert clean pass + document for P9 |
| `policy-widening-pr` | ✅ Yes — once P2 is done | Skip until P2, or assert no crash |
| `ownership-overlap` | ⚠️ Partially — `doctor` shows overlaps | Assert doctor output, not check output |
| `orphan-module` | ⚠️ Partially — spec with zero matching files | Assert some diagnostic |
| `barrel-re-export-chain` | ✅ Yes — specgate traces through re-exports | Assert violation if boundary crossed |
| `type-import-downgrade` | ❌ Not today — no type vs value tracking | Assert clean pass + document |
| `circular-via-re-export` | ✅ Yes — circular dep detection exists | Assert `no_circular_dependencies` violation |
| `aliased-deep-import` | ⚠️ Maybe — depends on resolver + tsconfig | Need to verify resolver handles this |
| `dynamic-import-evasion` | ❌ By design — static analysis only | Assert clean pass + explicit "won't fix" |
| `conditional-require` | ❌ By design — static analysis only | Assert clean pass + explicit "won't fix" |

**Fix required:** P3.3's test runner should use a richer assertion format than just `expected.json`. Each fixture should have an `expected.yml` that specifies:
```yaml
assertion: catch  # or: clean_pass, diagnostic, skip
violations:
  - rule: boundary.never_imports
    from_module: handlers
    to_module: database
gap_reason: null  # or: "dynamic imports are not statically analyzable"
future_priority: null  # or: P9
```

---

## Finding 5: 38-Task Blast Radius Problem

**Severity: HIGH — operational risk**

The plan has 38 tasks, with P1 alone having 12 sequential tasks. The plan says P1.1–P1.6 are "independent extractions" executed "in order because later moves reference earlier types." This means they're NOT independent — they're sequential with implicit dependencies.

### The failure cascade problem

If P1.6 (extract blast radius helpers) introduces a subtle bug — say, it accidentally changes visibility of `BlastRadiusData` (currently a private struct at line 709) from private to `pub(crate)`, and a later task (P1.8, expanding `check.rs`) starts importing it directly instead of through the intended path — this won't be caught until integration tests exercise the blast radius path. The gate (`cargo test`) will pass because the tests are end-to-end and the blast radius feature only activates with `--since`.

### Recommended checkpoints

The plan needs explicit verification gates, not just "cargo test passes":

1. **After P1.6:** Run `cargo test` AND verify `mod.rs` line count has decreased by the expected ~665 LOC. If it hasn't, something wasn't extracted.
2. **After P1.11:** `mod.rs` should be under 500 lines. If it's not, the moves were incomplete.
3. **After P1.12:** `mod.rs` should be under 100 lines. Run `cargo clippy` with extra lint checks for unused imports (which would indicate dead code left behind).
4. **Between every task:** `git diff --stat` should show the expected file changes. If unexpected files are modified, stop and investigate.

**Fix required:** Add explicit checkpoint assertions to the plan:
```
P1.6 checkpoint: mod.rs < 2,900 lines, blast.rs exists, cargo test passes
P1.11 checkpoint: mod.rs < 500 lines, all doctor types in doctor.rs, cargo test passes
P1.12 checkpoint: mod.rs < 100 lines, render/ exists with trait + 3 impls, cargo test passes
```

---

## Finding 6: P5–P9 Are Placeholders, Not Plans

**Severity: LOW — but the "38 tasks" count is misleading**

P5–P9 get 2–3 sentences each. They are NOT ready for autonomous implementation:

- **P5 (Ownership Registry):** Says "~300 LOC" but doesn't specify: How does overlap detection work? What's the output format for `specgate doctor ownership`? How does `strict_ownership` interact with existing `doctor` output? What happens when two specs have overlapping globs — which one wins?

- **P6 (Edge Classification):** Says "~200 LOC" but the `edge_classification` output format is specified while the implementation isn't. Where does the classification happen — in the graph builder, in the rules engine, or in the verdict builder?

- **P7 (Baseline v2):** Says "~150 LOC" but baseline schema changes require migration logic. What happens when a tool reads a v2 baseline? Does it fail on v1? Is there a migration path?

- **P8 (Visibility Model):** Says "~250 LOC" but `visibility: internal` requires namespace hierarchy semantics. What defines a "parent namespace"? Module IDs are flat strings today. Is `api/handlers` a child of `api`?

- **P9 (Import Hygiene):** Says "~200 LOC" but `deny_deep_imports` needs resolver integration. How do you know if `express/lib/router` is "deep"? Do you check `package.json` exports? What about packages without exports field?

**Recommendation:** Don't count P5–P9 in the task total. They need their own planning pass. The honest scope is: **P1 (12 tasks) + P2 (6 tasks) + P3 (4 tasks) + P4 (3 tasks) = 25 tasks**. P5–P9 are backlog items, not planned work.

---

## Finding 7: Missing Concern — `doctor.rs`'s `pub(super)` Visibility

**Severity: MEDIUM — plan omission**

`doctor.rs` line 5: `pub(super) fn handle_doctor(args: DoctorArgs) -> CliRunResult`

This function is `pub(super)`, meaning it's visible only within the `cli` module. This is fine today. But when P1.11 moves ~1,100 LOC of doctor-adjacent functions INTO `doctor.rs`, those functions currently have NO visibility modifier (they're private to `mod.rs`). Once they're in `doctor.rs`, they become private to `doctor.rs` — which is correct, but `doctor.rs`'s `use super::*` import of types from `mod.rs` only works because `mod.rs` is the parent module.

The plan doesn't address: after P1.11, `doctor.rs` will be ~1,250 lines (its current ~170 lines + ~1,100 moved). That's still a big file. Should there be a `doctor/` subdirectory with `mod.rs`, `compare.rs`, `trace.rs`?

---

## Finding 8: The Dependency Diagram Has a Timing Bug

**Severity: LOW — misleading but not blocking**

The execution diagram says:
```
P2 (Policy Governance) ──────── can start after P1 ──┘
                                (uses new render trait)
```

But P2 doesn't need the render trait. P2.4 adds a new CLI command (`policy-diff`) with its own output formatting. It uses `--format human|json|ndjson` — the same formats that exist today without the render trait. P2 depends on P1 being done (so the CLI is clean to add a new command), but specifically on P1's module structure, not on P1.12's render trait.

P4 (SARIF) depends on the render trait. The dependency should be:
```
P2 depends on: P1.1–P1.11 (module structure)
P4 depends on: P1.12 (render trait)
```

This matters for scheduling: P2 can start before P1.12 is done.

---

## Summary of Required Changes Before Autonomous Execution

### Must fix (blocking):

1. **P1 visibility strategy:** Every task must specify which items get `pub(crate)` vs `pub(super)` vs re-export in `mod.rs`. The current "just move it" language will produce compilation failures.

2. **P1 test migration plan:** The 3 private items tested directly in `mod tests` (`escape_yaml_double_quoted`, `parse_structured_trace_data`, `STRUCTURED_TRACE_SCHEMA_VERSION`) must be accounted for — either move the tests or change visibility.

3. **P1.12 redesign acknowledgment:** Flag P1.12 as new design work, define the trait interface with enough detail for implementation, and address the diff-mode rendering path.

4. **Checkpoint assertions:** Add measurable gate criteria after P1.6, P1.11, and P1.12 (line counts, file existence, clippy clean).

### Should fix (high value):

5. **P2.3 git optimization:** Use `git diff --name-only` to scope spec comparison.

6. **P3 fixture assertion format:** Replace vague "don't crash" with specific expected-behavior specifications per scenario.

7. **P5–P9 honest scoping:** Remove from task count, label as "backlog requiring separate planning."

### Nice to fix (low urgency):

8. **P1.11 doctor submodule:** Consider whether 1,250-line `doctor.rs` needs further splitting.

9. **Dependency diagram correction:** P2 depends on P1.1–P1.11, not P1.12.

---

*Review complete. The bones of this plan are good. The gap is in the details that Rust's type system will enforce whether the plan accounts for them or not.*
