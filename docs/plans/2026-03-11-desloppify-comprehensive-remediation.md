# Desloppify Comprehensive Remediation Plan

**Goal:** Clear the remaining high-signal `desloppify` backlog in `specgate` without regressing CLI contracts, deterministic output, or Rust test coverage.

**Architecture:** Work from the imported `desloppify` review queue outward: first regularize public result/request shapes, then tighten error contracts and ownership boundaries, then drain the remaining review/mechanical queue and rescan once the plan queue is clear. Preserve serialized output schemas unless a change is explicitly intentional and paired with fixture/test updates.

**Tech Stack:** Rust CLI, Clap, serde, schemars, cargo test, `desloppify 0.9.4`

---

### Task 1: Baseline Classification Result Object

**Parallel:** no  
**Blocked by:** none  
**Owned files:** `src/baseline/mod.rs`, `src/cli/check.rs`, `src/cli/baseline_cmd.rs`, `tests/baseline_metadata.rs`, `tests/mvp_gate_baseline.rs`

**Files:**
- Modify: `src/baseline/mod.rs`
- Modify: `src/cli/check.rs`
- Modify: `src/cli/baseline_cmd.rs`
- Test: `tests/baseline_metadata.rs`
- Test: `tests/mvp_gate_baseline.rs`

**Step 1: Add a shared result type**
Create a `ClassificationResult` struct in `src/baseline/mod.rs` with:
- `violations: Vec<FingerprintedViolation>`
- `stale_count: usize`
- `expired_count: usize`

**Step 2: Refactor the classification helpers**
Make one internal engine return `ClassificationResult`, and convert:
- `classify_violations`
- `classify_violations_with_stale`
- `classify_violations_with_options`

into thin adapters over that shared engine.

**Step 3: Stop conflating load and normalization**
Split baseline load semantics so the raw loader does not silently canonicalize beyond deserialization. Keep any normalization explicit at call sites that require it.

**Step 4: Update callers**
Replace tuple-position usage in `src/cli/check.rs` and `src/cli/baseline_cmd.rs` with named field access from `ClassificationResult`.

**Step 5: Verify**
Run:
```bash
cargo test baseline_metadata --quiet
cargo test mvp_gate_baseline --quiet
cargo test envelope_checks --quiet
```
Expected: all pass, no output-shape regressions.

### Task 2: Verdict Builder Request Shape

**Parallel:** yes  
**Blocked by:** Task 1  
**Owned files:** `src/verdict/mod.rs`, `src/cli/check.rs`, `src/cli/analysis.rs`, `tests/integration.rs`, `tests/edge_classification_integration.rs`

**Files:**
- Modify: `src/verdict/mod.rs`
- Modify: `src/cli/check.rs`
- Modify: `src/cli/analysis.rs`
- Test: `tests/integration.rs`
- Test: `tests/edge_classification_integration.rs`

**Step 1: Introduce a request/options object**
Add a `VerdictBuildRequest` or equivalent builder input in `src/verdict/mod.rs` to group:
- `project_root`
- `violations`
- `suppressed_violations`
- `metrics`
- `identity`
- `governance`
- `options`

**Step 2: Keep compatibility wrappers**
Retain `build_verdict`, `build_verdict_with_governance`, and `build_verdict_with_options` as narrow adapters over the new request path so current callers and tests stay stable.

**Step 3: Migrate internal callers**
Move the major internal call sites to the request struct so future growth no longer adds positional parameters.

**Step 4: Verify**
Run:
```bash
cargo test integration --quiet
cargo test edge_classification_integration --quiet
```
Expected: verdict JSON shape remains unchanged.

### Task 3: Workspace Discovery Error Contracts

**Parallel:** yes  
**Blocked by:** none  
**Owned files:** `src/spec/workspace_discovery.rs`, `src/spec/mod.rs`, `src/cli/project.rs`, `tests/monorepo_integration.rs`, `tests/tsjs_openclaw_regression.rs`

**Files:**
- Modify: `src/spec/workspace_discovery.rs`
- Modify: `src/spec/mod.rs`
- Modify: `src/cli/project.rs`
- Test: `tests/monorepo_integration.rs`
- Test: `tests/tsjs_openclaw_regression.rs`

**Step 1: Audit silent-empty paths**
Identify every path in `src/spec/workspace_discovery.rs` that currently returns `Vec::new()` on malformed config/glob/traversal input.

**Step 2: Introduce typed failures or warnings**
Align workspace discovery with sibling APIs by returning explicit typed failures or structured warnings instead of silently collapsing to empty results.

**Step 3: Preserve current successful behavior**
Keep “no workspace packages found” as a legitimate empty result; only error on malformed inputs and traversal failures that currently disappear.

**Step 4: Verify**
Run:
```bash
cargo test monorepo_integration --quiet
cargo test tsjs_openclaw_regression --quiet
```
Expected: healthy repos still pass; malformed workspace state becomes diagnosable.

### Task 4: CLI Root Decoupling and Shared Analysis Context

**Parallel:** no  
**Blocked by:** Task 1  
**Owned files:** `src/cli/mod.rs`, `src/cli/analysis.rs`, `src/cli/check.rs`, `src/cli/baseline_cmd.rs`, `src/cli/blast.rs`, `src/cli/doctor/compare.rs`, `src/cli/doctor/overview.rs`, `src/cli/project.rs`, `src/cli/util.rs`, `src/cli/tests.rs`

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/analysis.rs`
- Modify: `src/cli/check.rs`
- Modify: `src/cli/baseline_cmd.rs`
- Modify: `src/cli/blast.rs`
- Modify: `src/cli/doctor/compare.rs`
- Modify: `src/cli/doctor/overview.rs`
- Modify: `src/cli/project.rs`
- Modify: `src/cli/util.rs`
- Test: `src/cli/tests.rs`
- Test: `tests/integration.rs`
- Test: `tests/wave2c_cli_integration.rs`

**Step 1: Shrink `cli::mod`**
Reduce `src/cli/mod.rs` to command registration, shared argument structs, and truly crate-wide CLI types. Remove ambient re-export usage where practical.

**Step 2: Extract prepared analysis context**
Create one helper in `src/cli/analysis.rs` or `src/cli/project.rs` that owns:
- project load
- validation gate
- graph/resolver analysis
- layer-config issue checks

**Step 3: Migrate command handlers**
Switch `check`, `baseline`, `blast`, `doctor compare`, and `doctor overview` to the shared prepared-context helper instead of rebuilding the same pipeline inline.

**Step 4: Replace `super::*` usage**
In touched CLI files, replace umbrella imports with explicit module imports so file ownership is readable locally.

**Step 5: Verify**
Run:
```bash
cargo test src::cli::tests --quiet
cargo test integration --quiet
cargo test wave2c_cli_integration --quiet
```
Expected: CLI JSON and exit-code contracts remain stable.

### Task 5: Doctor/Resolver Honesty Pass

**Parallel:** no  
**Blocked by:** Task 4  
**Owned files:** `src/cli/doctor/parity.rs`, `src/cli/doctor/compare.rs`, `src/cli/doctor/canonical.rs`, `src/cli/doctor/focus.rs`, `src/cli/doctor/types.rs`, `src/resolver/mod.rs`, `tests/wave2c_cli_integration.rs`, `src/cli/doctor/tests.rs`

**Files:**
- Modify: `src/cli/doctor/parity.rs`
- Modify: `src/cli/doctor/compare.rs`
- Modify: `src/cli/doctor/canonical.rs`
- Modify: `src/cli/doctor/focus.rs`
- Modify: `src/cli/doctor/types.rs`
- Modify: `src/resolver/mod.rs`
- Test: `tests/wave2c_cli_integration.rs`
- Test: `src/cli/doctor/tests.rs`

**Step 1: Finish the parity follow-through**
Review the remaining imported issues around:
- focused parity fallback honesty
- mismatch category ordering
- canonical importer/probe semantics

The recent parity fixes are the base; this task finishes the remaining queue items in that subsystem.

**Step 2: Separate explain vs mutate semantics**
If `resolver::explain_resolution` mutates cache state, either:
- rename it to reflect mutation, or
- split cache-populating work from a read-only explanation API.

**Step 3: Replace remaining stringly finite states**
For doctor compare internals, use enums for finite status/category vocabularies while preserving serialized string output.

**Step 4: Verify**
Run:
```bash
cargo test wave2c_cli_integration --quiet
cargo test doctor_parity_fixtures --quiet
cargo test parity::tests --quiet
```
Expected: focused compare remains deterministic and more honest about uncertainty.

### Task 6: Spec/Rules Boundary Cleanup

**Parallel:** yes  
**Blocked by:** none  
**Owned files:** `src/spec/validation.rs`, `src/rules/mod.rs`, `src/spec/mod.rs`, `tests/contract_validation_fixtures.rs`, `tests/contracts_rules_contract_refs.rs`

**Files:**
- Modify: `src/spec/validation.rs`
- Modify: `src/rules/mod.rs`
- Modify: `src/spec/mod.rs`
- Test: `tests/contract_validation_fixtures.rs`
- Test: `tests/contracts_rules_contract_refs.rs`

**Step 1: Move shared rule IDs to a neutral leaf**
Extract shared rule ID constants into a leaf module with no reciprocal dependency pressure.

**Step 2: Update imports**
Make both spec validation and rules code depend on the neutral module instead of each other’s top-level modules.

**Step 3: Verify**
Run:
```bash
cargo test contract_validation_fixtures --quiet
cargo test contracts_rules_contract_refs --quiet
```

### Task 7: Mechanical Debt Sweep and Final Queue Drain

**Parallel:** no  
**Blocked by:** Tasks 1-6  
**Owned files:** `src/**`, `tests/**`, `.desloppify/**`

**Files:**
- Modify: backlog-dependent
- Verify: repository-wide

**Step 1: Work the remaining `desloppify` queue**
Use:
```bash
/tmp/desloppify-094-9u5XtM/venv/bin/desloppify next
/tmp/desloppify-094-9u5XtM/venv/bin/desloppify show review --status open
```
Drain workflow/triage items first, then remaining review issues in priority order.

**Step 2: Re-run triage stages when the queue expects it**
Use `desloppify plan triage ...` as required once objective backlog guards clear.

**Step 3: Final verification**
Run:
```bash
cargo test
/tmp/desloppify-094-9u5XtM/venv/bin/desloppify scan --force-rescan --attest "I understand this resets the mid-cycle queue and I am intentionally running the final closure scan after completing the queued work."
```

**Step 4: Capture closure**
Record:
- final strict/objective/verified scores
- remaining wontfix items
- any detector false positives worth upstream reporting

## Parallelization Notes

- Tasks 2, 3, and 6 can run in parallel if their owned files remain disjoint.
- Task 4 should stay serial because it cuts across multiple CLI entrypoints and shared helpers.
- Task 5 should follow Task 4 because doctor compare currently depends on the same CLI setup seams.

## Recommended Execution Order

1. Task 1
2. Tasks 2, 3, and 6 in parallel
3. Task 4
4. Task 5
5. Task 7

## Verification Checklist

- `cargo test` passes on the final branch
- `desloppify next` queue is empty before the final forced rescan
- `desloppify` strict score is higher than `65.3/100`
- CLI JSON snapshots and fixture/golden suites remain deterministic
