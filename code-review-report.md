# Specgate Code Review Report — Last 13 Commits

**Date:** 2026-03-12
**Reviewer:** 13 parallel Sonnet code-review agents, orchestrated by Opus
**Scope:** Commits `a92602a` through `0f50a73` on `master`
**Project:** Specgate — Rust CLI for module boundary enforcement in TS/JS projects

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Commit Inventory](#commit-inventory)
3. [Blocking Findings](#blocking-findings)
4. [Major Findings](#major-findings)
5. [Moderate Findings](#moderate-findings)
6. [Minor Findings](#minor-findings)
7. [Systemic Patterns](#systemic-patterns)
8. [Validation Artifact Quality](#validation-artifact-quality)
9. [Per-Commit Detailed Reviews](#per-commit-detailed-reviews)
10. [Recommended Action Plan](#recommended-action-plan)

---

## Executive Summary

13 commits were reviewed covering three milestones (rule-expansion, advanced-diagnostics, import-hygiene) plus documentation. The implementation quality of new Rust code is generally high — well-structured, deterministic, and well-tested. However, the review uncovered **2 critical issues**, **4 major issues**, and several moderate concerns that should be addressed before a release cut.

**Verdict breakdown:** 6 approve, 7 revise

The most impactful findings are:
- Silent glob validation gap that can neuter deny rules (security-relevant)
- Dead code path in governance-consistency that passes unit tests but is unreachable via CLI
- Incomplete pairwise comparison that silently misses conflicts
- Per-edge glob recompilation performance regression at scale
- Hardcoded severity across 3 rules that ignores user-declared constraint severity
- Path-boundary false positive in structural overlap detection
- Fabricated/truncated evidence in validation artifacts

---

## Commit Inventory

| # | SHA | Message | Type | Files Changed | Verdict |
|---|-----|---------|------|---------------|---------|
| 1 | `a92602a` | feat(rules): add C02 pattern-aware boundary rule variants | feature | 15+ | **REVISE** |
| 2 | `dfcf681` | feat(rules): add C06 category-level governance rule (enforce-category) | feature | 8+ | approve |
| 3 | `02081dd` | feat(rules): add C07 boundary.unique_export rule (enforce-unique-export) | feature | 10+ | **REVISE** |
| 4 | `624d40b` | chore(validation): scrutiny synthesis for rule-expansion milestone | validation | 5 | approve |
| 5 | `b56825d` | chore(validation): user testing validation for rule-expansion milestone | validation | 3 | approve |
| 6 | `2282c4e` | feat(doctor): add governance-consistency subcommand | feature | 2 | **REVISE** |
| 7 | `235fa86` | feat(doctor): add contradictory ownership glob detection | feature | 3 | **REVISE** |
| 8 | `c78c18e` | chore(validation): scrutiny synthesis for advanced-diagnostics milestone | validation | 3 | approve |
| 9 | `a31c3ce` | chore(validation): user testing validation for advanced-diagnostics milestone | validation | 4 | **REVISE** |
| 10 | `f50e9f6` | feat(hygiene): add deep internal import hygiene scenarios with Tier A fixture | feature | 15+ | approve |
| 11 | `c6e1898` | chore(validation): scrutiny synthesis for import-hygiene milestone | validation | 2 | approve |
| 12 | `6bbb7d5` | chore(validation): user testing validation for import-hygiene milestone | validation | 3 | **REVISE** |
| 13 | `0f50a73` | docs: update README with newly shipped v0.3.1 backlog features | docs | 1 | **REVISE** |

---

## Blocking Findings

### B1. Dead code: `duplicate_contract_id` detection unreachable via CLI

**Commit:** `2282c4e` (governance-consistency subcommand)
**File:** `src/cli/doctor/governance_consistency.rs` lines 200-217
**Cross-ref:** `src/spec/validation.rs` lines 181-199

**Problem:** The `detect_governance_conflicts()` function includes a `duplicate_contract_id` detection branch. However, `src/spec/validation.rs` already catches duplicate contract IDs within a module as a `push_error`. Since `handle_doctor_governance_consistency` bails out at lines 57-69 when `loaded.validation.has_errors()` is true, the `duplicate_contract_id` branch can **never fire** through the CLI path. The unit test at line 506 passes only because it calls `detect_governance_conflicts()` directly, bypassing the validation gate.

**Impact:** The feature described in the commit message is silently absent at the CLI level. Users cannot observe duplicate_contract_id conflicts via `specgate doctor governance-consistency` because the validation error aborts the command first.

**Fix options:**
1. Remove the dead branch and its test, documenting that validation already handles this case.
2. Repurpose the branch to detect **cross-module** duplicate contract IDs (two different modules publishing the same contract ID string), which validation does NOT catch.
3. Change the validation gate to allow governance-consistency to run even when validation has errors, filtering out the specific error types it can handle.

**Evidence:**
```rust
// src/spec/validation.rs:190-197 — already catches duplicate contract IDs as push_error
// src/cli/doctor/governance_consistency.rs:57-69 — bails on has_errors()
// src/cli/doctor/governance_consistency.rs:200-217 — duplicate_contract_id branch (unreachable)
// test at line 506 — bypasses CLI path, calls detect_governance_conflicts() directly
```

---

### B2. Incomplete pairwise comparison for imports_contract conflicts

**Commit:** `2282c4e` (governance-consistency subcommand)
**File:** `src/cli/doctor/governance_consistency.rs` lines 247-269

**Problem:** The cross-spec `imports_contract` conflict detection only compares each reference against `refs[0]` (the first consumer), not pairwise across all consumers:

```rust
let first = &refs[0];
for other in &refs[1..] {
    if first.direction != other.direction || first.envelope != other.envelope {
        // only catches A-vs-B, A-vs-C — misses B-vs-C
    }
}
```

Given consumers [A, B, C] where A and B agree but C conflicts with B (not A), the B-vs-C conflict is silently missed.

**Impact:** False negatives in governance conflict detection. Users with 3+ consumers referencing the same contract could have undetected policy conflicts.

**Fix:** Use a proper pairwise comparison (nested loop over `i < j` pairs) or group by `(direction, envelope)` and flag any group with >1 distinct value.

---

## Major Findings

### M1. Silent glob validation gap — false-pass security hole

**Commit:** `a92602a` (C02 pattern-aware boundaries)
**File:** `src/rules/boundary.rs` lines 484-489
**Cross-ref:** `src/spec/validation.rs` lines 335-342

**Problem:** When `Glob::new(trimmed)` fails in `matches_module_pattern`, the entry is silently skipped:

```rust
if let Ok(glob) = Glob::new(trimmed) {
    if glob.compile_matcher().is_match(module_id) { return true; }
}
```

The doc comment says "the user will receive separate config-validation feedback," but `src/spec/validation.rs` only validates glob syntax in `boundaries.path` and `boundaries.public_api`. The six fields changed by this commit — `never_imports`, `allow_imports_from`, `allow_type_imports_from`, `deny_imported_by`, `allow_imported_by`, `friend_modules` — are **NOT** validated.

**Impact:** A typo like `legacy/**{` in `never_imports` silently becomes a no-op. The deny rule is neutered with no user feedback. This is a false-pass security hole — the boundary enforcement silently degrades.

**Fix:** Add glob syntax validation for all six boundary entry fields in `src/spec/validation.rs`, mirroring the existing `public_api` glob validation at lines 335-342. Alternatively, emit a config issue from the hot path when `Glob::new()` fails.

**Additionally:** The existing exact-string overlap check in `validation.rs:357-361` (detecting modules in both `allow_imports_from` and `never_imports`) is now semantically broken for glob patterns. A glob entry like `shared/*` in `allow_imports_from` and `shared/utils` (exact) in `never_imports` are a semantic overlap that the validator will not flag. The validator uses exact BTreeSet intersection but runtime uses glob matching.

---

### M2. Per-edge glob recompilation — performance regression at scale

**Commit:** `a92602a` (C02 pattern-aware boundaries)
**File:** `src/rules/boundary.rs` lines 484-489

**Problem:** `matches_module_pattern` calls `Glob::new(trimmed).compile_matcher()` inside the hot edge-evaluation loop with no caching. For a project with N edges and M pattern-list entries containing globs, this is O(N×M) glob compilations per `check` run.

**Context:** The existing `BoundaryMatcherCache` already pre-compiles `public_api` globs for exactly this reason. The same approach should be applied to `never_imports`, `allow_imports_from`, `allow_type_imports_from`, `deny_imported_by`, `allow_imported_by`, and `friend_modules`.

**Impact:** At OpenClaw scale (referenced in `tests/perf_budget.rs`), this will register as a measurable regression. The fix is straightforward: store `Vec<CompiledPattern>` (exact string OR pre-compiled `GlobMatcher`) in `BoundaryMatcherCache` for every pattern-bearing field.

---

### M3. Hardcoded `Severity::Error` ignores spec-declared severity

**Commits:** `02081dd` (C07 unique_export), `dfcf681` (C06 enforce-category), pre-existing in enforce-layer
**File:** `src/cli/analysis.rs` lines 361 (C06), 390-391 (C07)

**Problem:** All three constraint rules (`enforce-layer`, `enforce-category`, `boundary.unique_export`) hardcode `Severity::Error` in the `PolicyViolation` mapping, ignoring the `severity` field declared on the user's `Constraint` in their spec file:

```rust
// analysis.rs line 390
PolicyViolation {
    severity: Severity::Error,  // HARDCODED — ignores constraint's severity field
    ...
}
```

Every other constraint rule in the codebase reads severity from the spec via `severity_for_constraint_rule()`. These three rules bypass this entirely.

**Impact:** A user who writes `severity: warning` in their spec constraint will still get an error-level violation and a non-zero exit code. This violates the constraint system's contract.

**Fix:** Look up the per-module spec and call `severity_for_constraint_rule(spec, RULE_ID)` before constructing the `PolicyViolation`, falling back to `Severity::Error` if no matching spec is found — the same pattern used by `boundary_violation_severity()`.

---

### M4. Path-boundary false positive in `globs_structurally_overlap()`

**Commit:** `235fa86` (contradictory ownership globs)
**File:** `src/spec/ownership.rs` lines 297-311

**Problem:** The `literal_prefix()` function extracts the non-glob prefix of a pattern, and `globs_structurally_overlap()` checks if one prefix starts with the other. This produces false positives at path boundaries:

```
literal_prefix("src/api*")    = "src/api"
literal_prefix("src/api-v2/**") = "src/api-v2/"
"src/api-v2/".starts_with("src/api") → TRUE (false positive!)
```

`src/api*` and `src/api-v2/**` are actually disjoint pattern spaces, but they are falsely reported as contradictory.

**Impact:** Users with patterns like `packages/app*` and `packages/app-utils/**` will get spurious contradiction warnings. Since contradictory globs are error-level findings that gate CI with `strict_ownership`, this can cause false CI failures.

**Fix:** Require a path-separator boundary in the `starts_with` comparison:

```rust
let (short, long) = if prefix_a.len() <= prefix_b.len() {
    (&prefix_a, &prefix_b)
} else {
    (&prefix_b, &prefix_a)
};
long.starts_with(short.as_str())
    && (long.len() == short.len()
        || long[short.len()..].starts_with('/'))
```

**Missing test coverage:** No test covers this edge case. The existing disjoint test uses `src/api/**` vs `src/ui/**` which avoids the ambiguity.

---

## Moderate Findings

### D1. Redundant `seen_pairs` BTreeSet is dead code

**Commit:** `235fa86`
**File:** `src/spec/ownership.rs` lines 221-237

The `i < j` loop invariant already guarantees each `(i, j)` pair is visited exactly once. The `seen_pairs.insert(pair_key)` call is inside the `if is_contradictory` block, meaning it's only inserted when the pair is contradictory. The `contains()` check at line 235 can therefore never be true for a non-contradictory pair. The entire BTreeSet is unreachable overhead. Remove it.

---

### D2. Config issues treated as fatal errors abort runs before violations are emitted

**Commits:** `dfcf681` (C06), `02081dd` (C07)
**File:** `src/cli/analysis.rs` lines 597-603

When two modules declare the same governance rule with different params, the conflict message says "using canonical config from module X" — implying the policy still executes. But `prepare_analysis_for_loaded` returns `Err` on any non-empty config issues, which means the entire analysis aborts. A user with two modules with differing `enforce-category` params gets a fatal error with zero diagnostic output.

This contradicts the stated conflict-resolution design ("choose canonical, report mismatches"). The `enforce-layer` rule has the same behavior — this commit mirrors it faithfully — but it's an architectural wart. Consider routing conflict-type issues to a non-fatal warning channel.

---

### D3. `pub(crate)` visibility inconsistency on doctor submodules

**Commit:** `2282c4e`
**File:** `src/cli/doctor/mod.rs` line 4

`governance_consistency` is declared `pub(crate) mod` while all other doctor submodules (`canonical`, `compare`, `focus`, `overview`, `ownership`, `parity`, `trace_io`, `trace_parser`, `trace_types`, `types`) are private `mod`. The `ownership` module achieves external access by placing its types in `src/spec/ownership` instead. Align with the existing pattern.

---

### D4. No CLI integration tests for governance-consistency

**Commit:** `2282c4e`
**File:** `src/cli/doctor/governance_consistency.rs`

The 11 unit tests exercise `detect_governance_conflicts()` and `render_human()` directly, but there is no test via the CLI path (`handle_doctor_governance_consistency` with a fixture project). Other doctor subcommands (notably `ownership`) have fixture-based integration tests using `crate::cli::run()`. This means the clap argument wiring, exit code behavior, and JSON serialization path are only verified manually, not in CI.

---

### D5. `Debug` format coupling in user-facing output

**Commit:** `2282c4e`
**File:** `src/cli/doctor/governance_consistency.rs` lines 235-236

`format!("{:?}", contract.direction)` and `format!("{:?}", contract.envelope)` use Rust's Debug format for both comparison and user-facing output. If `ContractDirection` or `EnvelopeRequirement` gain a `Display` impl or the Debug repr changes, these string-equality comparisons silently break. Use a stable representation (`Display`, or match on enum variants directly).

---

### D6. `dedup()` semantics mismatch with sort comparator

**Commits:** `dfcf681` (C06), `02081dd` (C07)
**Files:** `src/rules/categories.rs` line 450, `src/rules/unique_export.rs` line 148

The sort step uses `to_string()` for comparison (comparing serialized `serde_json::Value`), while `dedup()` uses `PartialEq` on the tuple. These two orderings are not guaranteed to agree — for example, JSON objects with different key insertion order can have different `Display` output but identical structural equality. The comment on line 130 says "keyed by params string for dedup" while the actual dedup does structural equality. Either sort and dedup using the same comparator, or use `dedup_by`.

---

### D7. `spec_path` in imports_contract_conflict only shows first consumer

**Commit:** `2282c4e`
**File:** `src/cli/doctor/governance_consistency.rs` line 265

The `spec_path` field in an `imports_contract_conflict` is always the first consumer's spec path. The other consumer's spec path is dropped entirely. Both paths should appear to aid navigation when the conflict spans two spec files.

---

### D8. Redundant overlap detection between governance-consistency and ownership

**Commit:** `2282c4e`
**File:** `src/cli/doctor/governance_consistency.rs` lines 106-142
**Cross-ref:** `src/spec/validation.rs` lines 344-380

Checks 1 (`allow_never_overlap`) and 2 (`allow_deny_imported_by_overlap`) in `detect_governance_conflicts` are logic-identical to the BTreeSet intersection logic in `src/spec/validation.rs`. The difference is that validation emits `push_warning` (non-blocking) while governance-consistency emits them as conflicts. This is defensible UX but should be documented as intentional, not accidental.

---

### D9. `PolicyViolation` `from_file`/`to_file` only captures first two files

**Commit:** `02081dd` (C07 unique_export)
**File:** `src/cli/analysis.rs` line 389

For `boundary.unique_export`, violations can involve 3+ files sharing a duplicate export. The mapping uses `files[0]` → `from_file` and `files[1]` → `to_file`, silently dropping `files[2..]`. The violation message string includes all file paths, so no information is lost in human output, but structured JSON consumers (editor annotations, SARIF) will only see the first two files.

---

### D10. Overlapping ownership displayed twice in different severity buckets

**Commit:** `235fa86`
**File:** `src/cli/doctor/ownership.rs` lines 42-55

When two modules have identical globs, the same files appear in both `overlapping_files` (warning) and `contradictory_globs` (error). The human renderer shows both sections. Users see the same problem surfaced twice in different severity buckets with no deduplication or cross-reference.

---

### D11. `is_ownership_validation_issue` uses fragile substring matching

**Commit:** `235fa86` (pre-existing, worsened)
**File:** `src/cli/doctor/ownership.rs` lines 65-67

The function matches validation messages by substring: `"duplicate module"` and `"invalid boundaries.path glob pattern"`. These strings are defined in `src/spec/validation.rs`. If those message strings change, the partition silently breaks. The new `contradictory_globs` category is NOT covered by `is_ownership_validation_issue` — if a future validation pass emits a "contradictory glob" message, it would become a blocking error rather than flowing through the report.

---

## Minor Findings

### N1. `contains_glob_meta` does not handle escape sequences

**Commit:** `a92602a`
**File:** `src/rules/boundary.rs`

A user who writes `\*` to mean a literal `*` will have it detected as a glob metacharacter and sent through `Glob::new()`, which will compile it as an escaped literal. Functionally correct but the function name implies "is a glob pattern" rather than "contains a metacharacter byte."

---

### N2. `has_module_patterns` duplicates trim logic

**Commit:** `a92602a`
**File:** `src/rules/boundary.rs`

Both `has_module_patterns` and `matches_module_pattern` trim each entry. A shared `non_empty_trimmed()` predicate would reduce the surface.

---

### N3. `fixture.meta.yml` `expected_count` field is unused

**Commits:** `a92602a`, `f50e9f6`
**File:** Various `fixture.meta.yml` files

The `expected_count` field is not used by the tier-A golden harness (which derives counts from `expected/{variant}.verdict.json`). Harmless but misleading.

---

### N4. `category_for_module` duplicates `layer_for_module`

**Commit:** `dfcf681`
**Files:** `src/rules/categories.rs:287-291`, `src/rules/layers.rs:139-143`

These are identical implementations. A shared helper in `src/rules/mod.rs` would reduce drift risk.

---

### N5. Missing `#[serde(skip_serializing_if = "Vec::is_empty")]` on doctor fields

**Commit:** `dfcf681`
**File:** `src/cli/doctor/types.rs` line 19

`category_config_issues` and `layer_config_issues` are missing this annotation that `unique_export_config_issues` has. The doctor JSON output always contains these keys as `[]` even when no rule is configured, which is a schema-breaking addition for downstream consumers.

---

### N6. `duplicate_contract_id` description leaks namespaced key

**Commit:** `2282c4e`
**File:** `src/cli/doctor/governance_consistency.rs` line 209

The description includes the full `module:contract_id` key (e.g., "Contract 'provider:dup_contract'"), redundant with the `module` field. Should show only the contract ID portion.

---

### N7. Schema version hardcoded inline

**Commits:** `2282c4e`, `235fa86`
**Files:** `src/cli/doctor/governance_consistency.rs` line 89, `src/cli/doctor/ownership.rs` line 126

`schema_version: "1.0".to_string()` is hardcoded inline. Other command outputs use named constants (e.g., `POLICY_DIFF_SCHEMA_VERSION`). Use a named constant for consistency.

---

### N8. Star re-export skip comment could be clearer

**Commit:** `02081dd`
**File:** `src/rules/unique_export.rs` line 169

The comment says star re-exports "don't introduce specific named bindings at the static level we can check." This could be read as implying the rule misses barrel-file duplicate exports. In fact, the originating files' direct exports ARE still checked. A clarifying comment would prevent confusion.

---

### N9. No test for cross-module duplicate export names (negative case)

**Commit:** `02081dd`

There is no test verifying that a duplicate export name shared between files in *different* modules does NOT trigger a violation. If `files_in_module` is ever misconfigured, the rule could produce cross-module false positives with no test to catch it.

---

### N10. Test name misrepresents what's being tested

**Commit:** `f50e9f6`
**File:** `src/rules/hygiene.rs` line 701

`bidirectional_mode_blocks_test_importing_deep_internal_non_public_files` implies the distinguishing property is bidirectional mode, but the existing test at line 537 already covers that. The new test's distinguishing property is path depth interacting with the `public_api` glob matcher. A name like `public_api_boundary_enforced_for_deeply_nested_internal_file` would be more accurate.

---

### N11. `zero_max_depth` test asserts count but not message content

**Commit:** `f50e9f6`
**File:** `src/rules/hygiene.rs` lines 664-698

The test asserts `violations.len() == 2` but does not check the content of either violation message. A specifier-extraction regression on scoped packages (`@scope/lib`) would go undetected. Add `violations.iter().any(|v| v.message.contains("@scope/lib/deep/nested/file"))`.

---

### N12. Tier A fixture violations are indistinguishable in contract shape

**Commit:** `f50e9f6`
**File:** `tests/fixtures/golden/tier-a/import-hygiene/expected/intro.verdict.json`

Both violation records are structurally identical (same `rule`, `from_module`, `to_module`). The harness's `canonical_violation_contract` drops `from_file`/`to_file`, so the contract assertion could pass even if the engine emits two violations for the wrong files. The violations are distinguishable by `to_file` (`format.ts` vs `token.ts`) but that field is invisible to the contract assertion.

---

### N13. README inaccuracies

**Commit:** `0f50a73`
**File:** `README.md`

1. **Line 173:** "contradictory namespace-intent across policies" is a marketing phrase not grounded in the implementation. The command detects 5 specific conflict categories. Replace with a concrete description.
2. **Line 174:** "C07 unique-export/visibility boundaries" misstates what C07 does. C07 enforces export-name uniqueness, not visibility. Replace with "C07 unique-export name enforcement within module boundaries."
3. **Line 175:** "Deep package-internal import hygiene scenario coverage" reads as a test addition, not a user-facing feature. The commit adds fixture coverage for `boundary.public_api`, not a new configurable rule.

---

## Systemic Patterns

### S1. Hardcoded severity across constraint rules

Three rules (`enforce-layer`, `enforce-category`, `boundary.unique_export`) all hardcode `Severity::Error` in their `PolicyViolation` mapping, ignoring the user's declared severity. This is a systemic violation of the constraint system's contract. A single fix — reading severity from the constraint spec — should be applied to all three simultaneously.

### S2. `skip_serializing_if` inconsistency on DoctorOutput fields

`unique_export_config_issues` has `#[serde(default, skip_serializing_if = "Vec::is_empty")]` but `layer_config_issues` and `category_config_issues` do not. New fields always appear as `[]` in JSON output even when no rule is configured, which breaks downstream consumers expecting a stable schema. Apply the annotation consistently to all config_issues fields.

### S3. Single-canonical-config conflict resolution is undocumented

Both `enforce-layer` and `enforce-category` use the same pattern: when multiple modules declare the same governance rule, the lexicographically first module ID's config wins. This architectural decision is not documented in AGENTS.md or anywhere user-facing. Before a third rule accidentally invents a different strategy, codify this as a project convention.

### S4. Validation artifacts contain fabricated evidence

Multiple user-testing validation commits contain evidence that cannot have been produced by the actual CLI binary:
- `6bbb7d5`: The `output` strings show `summary` objects with only `total_violations` and `error_violations`, but the real `VerdictSummary` struct serializes 8+ flattened fields from `AnonymizedTelemetrySummary`.
- `a31c3ce`: Most scenarios use `<fixture>` placeholders without recording actual fixture paths or captured output.

This undermines the value of the validation artifacts as audit evidence. If these are intended to be genuine CLI output, they should be regenerated from actual runs.

### S5. Doctor subcommand testing pattern inconsistency

The `ownership` doctor subcommand has proper CLI integration tests using `crate::cli::run()` with tempdir fixtures. The `governance-consistency` subcommand has only unit tests calling domain functions directly. The CLI routing, argument parsing, exit code behavior, and JSON serialization path are unverified in CI for governance-consistency.

### S6. File-witnessed overlap branch may be untestable

In `235fa86`, the `detect_contradictory_globs` function has three detection strategies: identical patterns, structural overlap, and file-witnessed overlap. The existing test for "file-witnessed" overlap (`test_contradictory_globs_witnessed_by_files`) actually triggers structural overlap first (because the literal prefixes share a common path), meaning the file-witnessed branch is never exercised in isolation. There is no test that reaches the file-witnessed fallback path.

---

## Validation Artifact Quality

### Assessment of `.factory/validation/` commits

| Commit | Milestone | Type | Quality |
|--------|-----------|------|---------|
| `624d40b` | rule-expansion | scrutiny | **Good** — accurate findings, one self-contradiction in synthesis (services.yaml "already exist" vs "were not pre-existing") |
| `b56825d` | rule-expansion | user-testing | **Good** — evidence cross-checks against fixtures, one gap (VAL-RULE-002 missing humanOutput block) |
| `c78c18e` | advanced-diagnostics | scrutiny | **Good** — accurate findings, one miscount (calls 5 CLI tests "integration tests" when they are unit tests) |
| `a31c3ce` | advanced-diagnostics | user-testing | **Weak** — `<fixture>` placeholders, only 2 of 5 governance-consistency conflict types tested, no testing of known code defects |
| `c6e1898` | import-hygiene | scrutiny | **Good** — accurate findings, synthesis correctly reflects review |
| `6bbb7d5` | import-hygiene | user-testing | **Weak** — fabricated CLI output that doesn't match actual binary output format, only 1 assertion for entire milestone |

### Specific validation artifact issues

1. **Fabricated evidence in `6bbb7d5`:** The `val-hyg-001.json` evidence shows truncated `summary` objects (`{"total_violations":2,"error_violations":2}`) that cannot be produced by the CLI. The actual `VerdictSummary` includes `new_violations`, `baseline_violations`, `new_error_violations`, `new_warning_violations`, `stale_baseline_entries`, `expired_baseline_entries`, `suppressed_violations`, `warning_violations`, plus top-level envelope fields (`verdict_schema`, `schema_version`, `tool_version`, `git_sha`, `config_hash`, `spec_hash`, `output_mode`, etc.).

2. **Placeholder fixtures in `a31c3ce`:** 4 of 5 governance-consistency scenarios use `<fixture>` without recording the fixture path or contents. Only scenario 5 (existing adversarial fixture) points to a real committed path. The evidence is non-reproducible.

3. **Coverage gaps in `a31c3ce`:** Only 2 of 5 governance-consistency conflict categories are tested (allow_never_overlap and private_with_allow_imported_by). Missing: allow_deny_imported_by_overlap, duplicate_contract_id, imports_contract_conflict. The known pairwise-comparison gap (3+ consumers) has no user-testing scenario.

4. **Self-contradiction in `624d40b`:** The c02 review states services.yaml/init.sh "were not pre-existing — they were bootstrapped by the worker." The synthesis rejection reason says "these files already exist and are functioning correctly." These are contradictory statements about the same files.

---

## Per-Commit Detailed Reviews

### Commit 1: `a92602a` — C02 pattern-aware boundary rule variants

**Verdict: REVISE**

**What it does:** Replaces exact-string matching in 6 boundary fields (`allow_imports_from`, `never_imports`, `allow_type_imports_from`, `deny_imported_by`, `allow_imported_by`, `friend_modules`) with glob-pattern-aware matching. Introduces `contains_glob_meta()`, `matches_module_pattern()`, and `has_module_patterns()` utility functions in `boundary.rs`. Uses the existing `globset` crate dependency.

**What's good:**
- Clean separation of concerns — 3 well-named utility functions
- Backward-compatible: plain entries still use exact matching
- 10 unit tests covering all boundary fields
- 4 CLI-level contract fixture tests
- Tier A golden fixture with intro/fix variants
- Also fixes 5 pre-existing clippy warnings

**Issues found:**
- **[MAJOR]** M1 — Silent glob validation gap (security-relevant)
- **[MAJOR]** M2 — Per-edge glob recompilation (performance)
- **[MAJOR]** Validation.rs overlap check is now semantically broken for glob patterns (exact BTreeSet intersection at runtime uses glob matching)
- **[MINOR]** N1 — `contains_glob_meta` doesn't handle escape sequences
- **[MINOR]** N2 — Duplicated trim logic
- **[NIT]** `.factory/skills/rust-worker/SKILL.md` references `--grep` flag that `cargo test` doesn't support
- **[NIT]** Several formatting-only changes bundled into the commit make auditing harder

---

### Commit 2: `dfcf681` — C06 category-level governance rule

**Verdict: APPROVE**

**What it does:** Implements `enforce-category` rule that isolates cross-category imports between defined member groups. 617-line new module `src/rules/categories.rs`. Follows the `enforce-layer` architecture closely.

**What's good:**
- Config parsing with 11 malformed-input rejection cases
- Deterministic sorting on violations, config issues, and conflict resolution
- 8 unit tests covering parsing, same-category allowance, cross-category flagging, non-member allowance, malformed config, conflict determinism, category extraction
- Tier A golden fixture with intro/fix variants
- Proper integration across analysis pipeline, doctor output, spec validation

**Issues found:**
- **[MAJOR]** M3 — Hardcoded `Severity::Error` (systemic, shared with enforce-layer and C07)
- **[MODERATE]** D2 — Config issues treated as fatal errors
- **[MODERATE]** D6 — `dedup()` semantics mismatch with sort comparator
- **[MINOR]** N4 — `category_for_module` duplicates `layer_for_module`
- **[MINOR]** N5 — Missing `skip_serializing_if` annotation
- **[NIT]** `usable_configs` sort comment doesn't document module-uniqueness invariant

---

### Commit 3: `02081dd` — C07 boundary.unique_export rule

**Verdict: REVISE**

**What it does:** Implements `boundary.unique_export` constraint rule that detects duplicate named exports across files within a module boundary. 567-line new module `src/rules/unique_export.rs`. Supports scoped export filtering via params, excludes `__default` pseudo-exports, handles re-exports (non-star).

**What's good:**
- 14 unit tests, 4 contract fixture E2E tests, Tier A golden fixture
- Deterministic violations sorted by module then export name
- Config issues properly propagated through AnalysisArtifacts and doctor output
- Golden corpus for c07-registry-collision appropriately updated

**Issues found:**
- **[MAJOR]** M3 — Hardcoded `Severity::Error` ignores spec-declared severity
- **[MODERATE]** D6 — `dedup()` semantics mismatch with sort comparator
- **[MODERATE]** D9 — `from_file`/`to_file` only captures first 2 of N files
- **[MINOR]** N8 — Star re-export skip comment could be clearer
- **[MINOR]** N9 — No negative test for cross-module duplicate exports
- **[NIT]** Exported types could be `pub(crate)` instead of `pub`
- **[NIT]** Golden fixture spec files have empty `boundaries:` key with no path

---

### Commit 4: `624d40b` — Scrutiny synthesis for rule-expansion

**Verdict: APPROVE**

**What it does:** Adds scrutiny review JSON files for all 3 rule-expansion features (C02, C06, C07) plus a library document for adding new rules.

**Assessment:** Review findings are accurate and well-evidenced. The library doc correctly captures all 6 integration points for new rules. Two suggested AGENTS.md updates (glob pattern support in boundary fields, single-canonical-config convention) are appropriate.

**One inaccuracy:** The synthesis rejects the services.yaml observation saying "these files already exist" while the c02 review itself says "these files were not pre-existing — they were bootstrapped by the worker."

---

### Commit 5: `b56825d` — User testing validation for rule-expansion

**Verdict: APPROVE**

**What it does:** Adds user testing guide and flow test results for the rule-expansion milestone. 3 assertions (VAL-RULE-001, VAL-RULE-002, VAL-RULE-003), all passing.

**Assessment:** Evidence cross-checks against actual fixtures. Rule names, module names, file paths, exit codes, and violation counts all match. VAL-RULE-001 includes a `humanOutput` block verifying human-readable format; VAL-RULE-002 is missing this parity (minor gap). The `from_module == to_module` in VAL-RULE-003 is correct for an intra-module rule but not called out.

---

### Commit 6: `2282c4e` — Governance-consistency subcommand

**Verdict: REVISE**

**What it does:** Adds `specgate doctor governance-consistency` subcommand. 596-line new module detecting 5 categories of contradictory governance: allow/never overlap, allow/deny imported_by overlap, private with allow_imported_by, duplicate contract IDs, conflicting imports_contract references. Both JSON and human output formats.

**What's good:**
- BTreeMap for deterministic ordering
- Sort + dedup on conflicts
- 11 unit tests covering all conflict types + edge cases
- Handles error cases (project load failure, validation errors) consistently

**Issues found:**
- **[BLOCKING]** B1 — `duplicate_contract_id` detection is dead code at CLI level
- **[BLOCKING]** B2 — Incomplete pairwise comparison for imports_contract conflicts
- **[MODERATE]** D3 — `pub(crate)` visibility inconsistency
- **[MODERATE]** D4 — No CLI integration tests
- **[MODERATE]** D5 — `Debug` format coupling in user-facing output
- **[MODERATE]** D7 — `spec_path` only shows first consumer
- **[MODERATE]** D8 — Redundant overlap detection with validation.rs
- **[MINOR]** N6 — Description leaks namespaced key
- **[MINOR]** N7 — Schema version hardcoded inline

---

### Commit 7: `235fa86` — Contradictory ownership glob detection

**Verdict: REVISE**

**What it does:** Adds contradictory ownership glob detection to `specgate doctor ownership` via three strategies: identical patterns, structural prefix overlap, file-witnessed overlap. New `ContradictoryGlob` type in `src/spec/ownership.rs`. Classified as error-level finding for strict_ownership gating.

**What's good:**
- Three-tier detection strategy (identical → structural → file-witnessed)
- Deterministic ordering of contradictions
- 10 unit tests + 5 CLI integration tests
- Properly updates existing test that would have broken
- Error-level classification for CI gating

**Issues found:**
- **[MAJOR]** M4 — Path-boundary false positive in `globs_structurally_overlap()`
- **[MODERATE]** D1 — `seen_pairs` BTreeSet is dead code
- **[MODERATE]** D10 — Overlapping ownership displayed twice in different severity buckets
- **[MODERATE]** D11 — `is_ownership_validation_issue` uses fragile substring matching
- **[MODERATE]** S6 — File-witnessed overlap branch may be untestable as currently structured
- **[MINOR]** N7 — Schema version hardcoded inline
- **[NIT]** O(n² × m) complexity on large repos (document the fallback path complexity)

---

### Commit 8: `c78c18e` — Scrutiny synthesis for advanced-diagnostics

**Verdict: APPROVE**

**Assessment:** Accurate and complete. All non-blocking severity classifications are correct. The synthesis faithfully aggregates both reviews. One minor inaccuracy: the ownership-globs review mislabels 5 tests in `src/cli/doctor/ownership.rs` as "integration tests" when they are actually unit tests (they live in the same crate, not in `tests/`).

---

### Commit 9: `a31c3ce` — User testing for advanced-diagnostics

**Verdict: REVISE**

**Assessment:** Only 2 of 5 governance-consistency conflict categories are tested. Most scenarios use `<fixture>` placeholders without recording actual fixture paths. Exit code behavior is only captured in 1 of 4 governance-consistency scenarios. Evidence is non-reproducible. Known code defects (pairwise comparison gap, structural overlap false positives) have no corresponding test scenarios.

---

### Commit 10: `f50e9f6` — Import hygiene scenarios + Tier A fixture

**Verdict: APPROVE**

**What it does:** Adds 4 unit tests in `hygiene.rs` for deep import scenarios plus a Tier A golden fixture for `boundary.public_api` deep-path enforcement.

**What's good:**
- Tier A fixture correctly validates intro (2 deep-internal-file violations) and fix (routes through public API)
- Unit tests cover: deep third-party imports, scoped packages with max_depth=0, bidirectional mode deep paths, subpath_depth utility
- No new production logic needed — only test coverage for existing rules with deeper nesting

**Issues found:**
- **[MINOR]** N10 — Test name misrepresents what's being tested
- **[MINOR]** N11 — `zero_max_depth` test asserts count but not message content
- **[MINOR]** N12 — Tier A fixture violations are indistinguishable in contract shape
- **[NIT]** `subpath_depth` test could cover trailing-slash and single-slash edge cases

---

### Commit 11: `c6e1898` — Scrutiny synthesis for import-hygiene

**Verdict: APPROVE**

**Assessment:** Findings are accurate. The two non-blocking issues (duplicate violation objects in intro.verdict.json, test name/rule ID mismatch) are correctly classified. Synthesis correctly reflects the review with appropriate rejection reasoning for shared-state observations.

---

### Commit 12: `6bbb7d5` — User testing for import-hygiene

**Verdict: REVISE**

**Assessment:** The evidence output is fabricated — the `summary` objects shown cannot be produced by the actual CLI binary. The violation `message` field in the evidence is not part of the golden fixture contract shape. Only 1 assertion covers the entire milestone, and it only exercises `boundary.public_api`, leaving the hygiene rule engine itself unvalidated at the user-testing layer. Empty `frictions` and `blockers` arrays on a first-round test are suspicious.

---

### Commit 13: `0f50a73` — README update

**Verdict: REVISE**

**What it does:** Updates README.md to document v0.3.1 features. Adds `contradictory ownership globs` to strict_ownership_level description. Adds 3 new Project Status bullets.

**Issues found:**
- **[WARNING]** N13.1 — "contradictory namespace-intent across policies" is vague marketing language that doesn't describe the 5 specific conflict types
- **[WARNING]** N13.2 — "C07 unique-export/visibility boundaries" misstates what C07 does (it's export-name uniqueness, not visibility enforcement)
- **[WARNING]** N13.3 — Import Hygiene bullet reads as test coverage, not a user-facing feature
- **[NIT]** Project Status date is 2026-03-11 but commit was authored 2026-03-12

---

## Recommended Action Plan

### Priority 1 — Blocking (fix before release)

| # | Finding | Files | Effort | Status |
|---|---------|-------|--------|--------|
| 1 | B1: Remove or repurpose dead `duplicate_contract_id` branch | `governance_consistency.rs` | S | DONE — repurposed to cross-module detection, 3 tests |
| 2 | B2: Fix pairwise comparison for imports_contract conflicts | `governance_consistency.rs:247-269` | S | DONE — proper i<j pairwise loop, 1 test |
| 3 | M1: Add glob validation for 6 boundary entry fields | `src/spec/validation.rs` | M | DONE — push_warning for all 6 fields, 6 tests |
| 4 | M4: Fix path-boundary false positive in structural overlap | `src/spec/ownership.rs:297-311` | S | DONE — path-separator boundary check, 5 tests + D1 dead code removed |

### Priority 2 — Major (fix before next minor release)

| # | Finding | Files | Effort |
|---|---------|-------|--------|
| 5 | M3: Fix hardcoded severity across 3 rules | `src/cli/analysis.rs` (3 locations) | M |
| 6 | M2: Pre-compile glob patterns in BoundaryMatcherCache | `src/rules/boundary.rs` | M |
| 7 | D4: Add CLI integration tests for governance-consistency | `src/cli/doctor/governance_consistency.rs` | M |
| 8 | N13: Fix README inaccuracies | `README.md` | S |

### Priority 3 — Moderate (address in next sprint)

| # | Finding | Files | Effort |
|---|---------|-------|--------|
| 9 | D1: Remove dead `seen_pairs` BTreeSet | `src/spec/ownership.rs` | S | DONE (fixed with M4) |
| 10 | D3: Fix `pub(crate)` to private on governance_consistency module | `src/cli/doctor/mod.rs` | S |
| 11 | D5: Replace Debug format with Display for user-facing output | `governance_consistency.rs` | S |
| 12 | N5: Add `skip_serializing_if` to all config_issues fields | `src/cli/doctor/types.rs` | S |
| 13 | D6: Align sort and dedup comparators | `categories.rs`, `unique_export.rs` | S |
| 14 | S3: Document single-canonical-config convention in AGENTS.md | `AGENTS.md` | S |
| 15 | S4: Regenerate validation artifacts with actual CLI output | `.factory/validation/` | M |

### Priority 4 — Nice-to-have

| # | Finding | Files | Effort |
|---|---------|-------|--------|
| 16 | N4: Extract shared `category_for_module`/`layer_for_module` | `src/rules/mod.rs` | S |
| 17 | N9: Add negative test for cross-module duplicate exports | `tests/` | S |
| 18 | N10: Rename misleading test | `src/rules/hygiene.rs` | S |
| 19 | N11: Add message assertions to `zero_max_depth` test | `src/rules/hygiene.rs` | S |
| 20 | Add test for file-witnessed overlap branch in isolation | `src/spec/ownership.rs` | S |
| 21 | Add test for `src/api*` vs `src/api-v2/**` false-positive case | `src/spec/ownership.rs` | S | DONE (fixed with M4) |

**Effort key:** S = small (< 30 min), M = medium (1-3 hours)

---

*Report generated by 13 parallel Sonnet code-review agents orchestrated by Opus. Each commit was reviewed independently with full diff context and cross-referenced against the codebase.*
