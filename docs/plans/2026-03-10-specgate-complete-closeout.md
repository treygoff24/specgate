# Specgate Complete Closeout Implementation Plan

> **For Claude:** Spawn `task-builder` agent to implement this plan task-by-task.

**Goal:** Finish the remaining product, governance, adoption, documentation, and release work so Specgate reaches a coherent "done" state against its MVP vision, post-MVP hardening plan, and operator rollout goals.

**Architecture:** Treat this as a closeout program, not a single feature. First align repo truth and docs, then finish the remaining governance gaps, then tighten adoption and release operations, and finally ship a fully validated release candidate with all contracts, docs, and gates in sync.

**Tech Stack:** Rust 2024, clap, serde/serde_json, yaml_serde, git subprocesses, existing `cargo test` contract suites, GitHub Actions workflows, Markdown docs.

---

## Definition of Done

Specgate is "100% finished out" for this phase when all of the following are true:

1. The codebase, README, operator guide, changelog, and plans all describe the same current product state.
2. Remaining deferred governance gaps that block the product narrative are either implemented or explicitly moved into a named 1.0+ backlog document.
3. Operator adoption docs match the real CI workflow and the real command surface.
4. Required test gates pass: `cargo test`, `cargo test contract_fixtures`, `cargo test tier_a_golden`, `cargo test golden_corpus`.
5. Release docs, examples, and changelog are ready for the next tagged release without hidden manual steps.

---

## Execution Waves

| Wave | Theme | Why first |
|------|-------|-----------|
| 0 | Truth audit | Prevent planning against stale docs or assumptions |
| 1 | Narrative and docs alignment | Fixes current "lost big picture" problem immediately |
| 2 | Remaining product gaps | Completes the most visible unfinished capabilities |
| 3 | Adoption and rollout closeout | Converts features into usable operator workflows |
| 4 | Release hardening | Produces a shippable, coherent release |

---

## Wave 0 - Truth Audit

### Task 0.1: Create a live completion matrix

**Parallel:** no
**Blocked by:** none
**Owned files:** `docs/plans/2026-03-10-specgate-complete-closeout.md`, `docs/project-status.md`

**Files:**
- Create: `docs/project-status.md`
- Modify: `docs/plans/2026-03-10-specgate-complete-closeout.md`
- Read first: `README.md`, `CHANGELOG.md`, `docs/reference/operator-guide.md`, `docs/plans/hardening-phase.md`, `docs/plans/hardening-implementation.md`, `docs/plans/p2-policy-governance.md`

**Step 1: Write the failing documentation test target**

Create a checklist section in `docs/project-status.md` with these rows and initial `TODO` markers:

```md
| Area | Planned | Landed | Verified by | Status |
|------|---------|--------|-------------|--------|
| Phase 5 envelope check | yes | yes | cargo test rules/contracts | TODO |
| CLI refactor | yes | yes | cargo test cli | TODO |
| policy-diff | yes | yes | cargo test policy_diff_integration | TODO |
| SARIF output | yes | yes | cargo test verdict::format | TODO |
| doctor ownership | yes | yes | cargo test doctor ownership | TODO |
| adoption workflow docs | yes | partial | docs review | TODO |
```

**Step 2: Run verification commands and capture evidence**

Run:

```bash
cargo test policy_diff_integration
cargo test contract_fixtures
cargo test tier_a_golden
```

Expected: PASS, or clear failures that identify incomplete work.

**Step 3: Fill the matrix with exact repo truth**

Update each row to `done`, `partial`, or `open`, with one sentence of evidence and one sentence of remaining work.

**Step 4: Re-run the relevant tests if any status changed during investigation**

Run:

```bash
cargo test
```

Expected: PASS.

**Step 5: Commit**

```bash
git add docs/project-status.md docs/plans/2026-03-10-specgate-complete-closeout.md
git commit -m "docs: add live project completion matrix"
```

---

## Wave 1 - Narrative and Documentation Alignment

### Task 1.1: Align README project status with actual shipped capabilities

**Parallel:** yes
**Blocked by:** Task 0.1
**Owned files:** `README.md`

**Files:**
- Modify: `README.md:123`
- Test: `README.md`

**Step 1: Write the failing doc diff**

Replace the stale status block with a current state section that explicitly says:

```md
## Project Status

**Status (as of 2026-03-10): core MVP, Phase 5 envelope checks, policy governance MVP, monorepo support, SARIF, adversarial fixtures, and ownership diagnostics are implemented on `master`; remaining work is release-story alignment, operator adoption closeout, and selected governance follow-through.**
```

**Step 2: Review the existing README copy against `CHANGELOG.md` and `docs/project-status.md`**

Expected: identify stale references to post-MVP items that have already landed.

**Step 3: Make the minimal README edits**

Update only the status bullets and next-step bullets. Do not rewrite the whole README.

**Step 4: Verify docs remain consistent**

Run:

```bash
rg "Project Status|Post-MVP|policy-diff|SARIF|ownership" README.md CHANGELOG.md docs/project-status.md docs/reference/operator-guide.md
```

Expected: wording is consistent and no major contradiction remains.

**Step 5: Return diff summary for orchestrator review (no commit)**

---

### Task 1.2: Align operator guide with current product state

**Parallel:** yes
**Blocked by:** Task 0.1
**Owned files:** `docs/reference/operator-guide.md`

**Files:**
- Modify: `docs/reference/operator-guide.md:299`
- Test: `docs/reference/operator-guide.md`

**Step 1: Write the failing doc assertions**

Add/update bullets so the guide explicitly states:

```md
- `policy-diff` is available today and is the current CI path for governance checks.
- `check --deny-widenings` is not implemented yet; do not document it as available.
- SARIF output is available today via `specgate check --format sarif`.
- `doctor ownership` is available today and can be CI-gated with `strict_ownership: true`.
```

**Step 2: Verify current command surface**

Run:

```bash
cargo test cli::tests -- --nocapture
```

Expected: command parsing tests pass.

**Step 3: Update the guide minimally**

Keep existing structure; only refresh the status and next priorities sections.

**Step 4: Verify references**

Run:

```bash
rg "deny-widenings|policy-diff|sarif|doctor ownership" docs/reference/operator-guide.md docs/reference/policy-diff.md
```

Expected: no doc claims a missing command already exists.

**Step 5: Return diff summary for orchestrator review (no commit)**

---

### Task 1.3: Publish a single-source roadmap and retire stale references

**Parallel:** no
**Blocked by:** Task 1.1, Task 1.2
**Owned files:** `docs/reference/operator-guide.md`, `README.md`, `docs/project-status.md`, `docs/roadmap.md`

**Files:**
- Create: `docs/roadmap.md`
- Modify: `README.md`, `docs/reference/operator-guide.md`, `docs/project-status.md`

**Step 1: Write the failing roadmap structure**

Create `docs/roadmap.md` with these sections:

```md
## Landed
## In Progress
## Remaining to Call This Release Complete
## Explicitly Deferred Beyond This Release
```

**Step 2: Populate from repo truth only**

Use `CHANGELOG.md`, `docs/project-status.md`, and the open gaps in this plan.

**Step 3: Update cross-links**

Add one link from `README.md` and one from `docs/reference/operator-guide.md` to `docs/roadmap.md`.

**Step 4: Verify no contradictory roadmap source remains**

Run:

```bash
rg "as of 2026|post-MVP|Priority 1|Priority 2|Phase 5 complete" README.md docs/reference docs/plans docs/project-status.md docs/roadmap.md
```

Expected: old plan docs remain historical, while operator-facing docs point to the new roadmap.

**Step 5: Commit**

```bash
git add README.md docs/reference/operator-guide.md docs/project-status.md docs/roadmap.md
git commit -m "docs: unify project status and roadmap"
```

---

## Wave 2 - Remaining Product Gaps

### Task 2.1: Finish governance integration with `check --deny-widenings`

**Parallel:** no
**Blocked by:** Task 0.1
**Owned files:** `src/cli/check.rs`, `src/cli/mod.rs`, `src/cli/tests.rs`, `src/policy/mod.rs`, `src/policy/tests.rs`, `docs/reference/policy-diff.md`, `README.md`

**Files:**
- Modify: `src/cli/check.rs`, `src/cli/mod.rs`, `src/cli/tests.rs`, `src/policy/mod.rs`, `src/policy/tests.rs`, `docs/reference/policy-diff.md`, `README.md`
- Test: `src/cli/tests.rs`, `src/policy/tests.rs`, `tests/policy_diff_integration.rs`

**Step 1: Write the failing CLI test**

Add a test in `src/cli/tests.rs` like:

```rust
#[test]
fn check_rejects_policy_widenings_when_flag_is_enabled() {
    let result = run_for_test(&[
        "specgate",
        "check",
        "--deny-widenings",
        "--project-root",
        fixture_root,
    ]);
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.contains("policy widening"));
}
```

**Step 2: Run only the new failing test**

Run:

```bash
cargo test check_rejects_policy_widenings_when_flag_is_enabled -- --exact
```

Expected: FAIL because the flag does not exist or does nothing.

**Step 3: Implement the minimal flag plumbing**

Add a field like:

```rust
#[arg(long, default_value_t = false)]
pub deny_widenings: bool,
```

Then call the existing `policy` diff API during `check` only when the flag is set. Reuse the existing exit-2 runtime behavior. Do not duplicate classification logic.

**Step 4: Add policy-domain tests**

Add or update a unit test verifying `check` consumes the same summary signal used by `policy-diff`.

**Step 5: Run focused and broad tests**

Run:

```bash
cargo test policy_diff_integration
cargo test cli::tests
cargo test
```

Expected: PASS.

**Step 6: Update docs**

Document the new flag in `docs/reference/policy-diff.md` and `README.md`.

**Step 7: Commit**

```bash
git add src/cli/check.rs src/cli/mod.rs src/cli/tests.rs src/policy/mod.rs src/policy/tests.rs docs/reference/policy-diff.md README.md
git commit -m "feat(governance): add deny-widenings check integration"
```

---

### Task 2.2: Implement semantic rename pairing for policy diff

**Parallel:** no
**Blocked by:** Task 2.1
**Owned files:** `src/policy/git.rs`, `src/policy/classify.rs`, `src/policy/types.rs`, `src/policy/tests.rs`, `tests/policy_diff_integration.rs`, `docs/reference/policy-diff.md`

**Files:**
- Modify: `src/policy/git.rs`, `src/policy/classify.rs`, `src/policy/types.rs`, `src/policy/tests.rs`, `tests/policy_diff_integration.rs`, `docs/reference/policy-diff.md`

**Step 1: Write the failing integration test**

Add a repo-fixture test like:

```rust
#[test]
fn policy_diff_semantic_rename_without_policy_change_is_structural() {
    let report = run_policy_diff_fixture("rename-only-equivalent");
    assert!(!report.summary.has_widening);
    assert_eq!(report.summary.structural_changes, 1);
}
```

**Step 2: Run the new failing test**

Run:

```bash
cargo test policy_diff_semantic_rename_without_policy_change_is_structural -- --exact
```

Expected: FAIL because rename/copy is still fail-closed widening.

**Step 3: Implement the minimal rename pairing logic**

Use blob contents from old/new paths. If parsed specs are semantically equivalent after normalization, downgrade rename/copy from widening-risk to structural. If parsing fails or equivalence is unclear, remain fail-closed.

**Step 4: Add regression tests for unsafe cases**

Cover:

```rust
// rename + widened allow_imports_from => widening
// rename + parse failure => runtime error or widening-risk, whichever matches existing contract
// copy + equivalent content => still structural only if explicitly supported; otherwise keep fail-closed
```

**Step 5: Run verification**

Run:

```bash
cargo test policy_diff_integration
cargo test src::policy --lib
```

Expected: PASS.

**Step 6: Update docs**

Remove or narrow the "rename always widens" wording in `docs/reference/policy-diff.md`.

**Step 7: Commit**

```bash
git add src/policy/git.rs src/policy/classify.rs src/policy/types.rs src/policy/tests.rs tests/policy_diff_integration.rs docs/reference/policy-diff.md
git commit -m "feat(policy-diff): classify semantic renames safely"
```

---

### Task 2.3: Decide and document config-level governance scope

**Parallel:** no
**Blocked by:** Task 2.2
**Owned files:** `docs/reference/policy-diff.md`, `docs/roadmap.md`, `docs/project-status.md`, `CHANGELOG.md`

**Files:**
- Modify: `docs/reference/policy-diff.md`, `docs/roadmap.md`, `docs/project-status.md`, `CHANGELOG.md`

**Step 1: Write the failing decision record**

Add a section to `docs/reference/policy-diff.md`:

```md
## Config Governance Scope

Current release scope: `.spec.yml` files only.
Explicit non-scope: `specgate.config.yml` semantic diffing.
Reason: different risk model, separate change vocabulary, separate review ergonomics.
```

**Step 2: Review whether config diffing is release-blocking**

If yes, replace this task with a new implementation task before proceeding. If no, document it as a named follow-up in `docs/roadmap.md`.

**Step 3: Update status docs**

Mark this as `deferred by decision`, not `unknown`.

**Step 4: Verify wording consistency**

Run:

```bash
rg "config-level governance|specgate.config.yml diffing|deferred" docs/reference/policy-diff.md docs/roadmap.md docs/project-status.md CHANGELOG.md
```

Expected: one clear story.

**Step 5: Commit**

```bash
git add docs/reference/policy-diff.md docs/roadmap.md docs/project-status.md CHANGELOG.md
git commit -m "docs(governance): record config diff scope decision"
```

---

## Wave 3 - Adoption and Rollout Closeout

### Task 3.1: Make the canonical GitHub Actions example match the real current product

**Parallel:** yes
**Blocked by:** Task 1.3
**Owned files:** `examples/specgate-consumer-github-actions.yml`, `docs/design/ci-gate-understanding.md`, `docs/dogfood/rollout-checklist.md`

**Files:**
- Modify: `examples/specgate-consumer-github-actions.yml`, `docs/design/ci-gate-understanding.md`, `docs/dogfood/rollout-checklist.md`

**Step 1: Write the failing workflow assertions**

The example workflow should include, or explicitly explain omission of:

```yaml
- name: Specgate check
  run: specgate check --output-mode deterministic

- name: Policy governance
  run: specgate policy-diff --base origin/main

- name: Upload SARIF
  if: always()
  uses: github/codeql-action/upload-sarif@v3
```

**Step 2: Verify the commands are real**

Run the relevant CLI tests and inspect the example workflow.

**Step 3: Update docs and example together**

Do not document policy-diff or SARIF in isolation; keep workflow, CI guide, and rollout checklist synced.

**Step 4: Verify cross-file consistency**

Run:

```bash
rg "policy-diff|sarif|fetch-depth|upload-sarif" examples/specgate-consumer-github-actions.yml docs/design/ci-gate-understanding.md docs/dogfood/rollout-checklist.md
```

Expected: no contradictions.

**Step 5: Return diff summary for orchestrator review (no commit)**

---

### Task 3.2: Add operator-facing release readiness checklist

**Parallel:** yes
**Blocked by:** Task 1.3
**Owned files:** `RELEASING.md`, `docs/dogfood/release-channel.md`, `docs/dogfood/success-metrics.md`

**Files:**
- Modify: `RELEASING.md`, `docs/dogfood/release-channel.md`, `docs/dogfood/success-metrics.md`

**Step 1: Write the failing checklist content**

Add a short checklist to `RELEASING.md`:

```md
- Docs aligned (`README.md`, operator guide, roadmap, changelog)
- Required test suites green
- Example workflow up to date
- SARIF output verified against GitHub upload example
- Governance command behavior documented and tested
```

**Step 2: Update dogfood metrics wording if needed**

Ensure release promotion criteria mention governance and ownership diagnostics where appropriate.

**Step 3: Verify docs consistency**

Run:

```bash
rg "promotion|release|SARIF|policy-diff|ownership" RELEASING.md docs/dogfood/release-channel.md docs/dogfood/success-metrics.md
```

Expected: one consistent promotion story.

**Step 4: Return diff summary for orchestrator review (no commit)**

---

### Task 3.3: Merge the adoption docs updates and validate end-to-end narrative

**Parallel:** no
**Blocked by:** Task 3.1, Task 3.2
**Owned files:** `examples/specgate-consumer-github-actions.yml`, `docs/design/ci-gate-understanding.md`, `docs/dogfood/rollout-checklist.md`, `RELEASING.md`, `docs/dogfood/release-channel.md`, `docs/dogfood/success-metrics.md`

**Files:**
- Modify: all files above if review changes are needed

**Step 1: Review both doc diffs together**

Check for duplicate or conflicting guidance.

**Step 2: Make only the minimal merge fixes**

Prefer linking over repeating the same rules.

**Step 3: Validate the narrative manually**

Read in this order:

```text
README.md
docs/reference/operator-guide.md
docs/design/ci-gate-understanding.md
examples/specgate-consumer-github-actions.yml
RELEASING.md
```

Expected: the story reads cleanly start-to-finish.

**Step 4: Commit**

```bash
git add examples/specgate-consumer-github-actions.yml docs/design/ci-gate-understanding.md docs/dogfood/rollout-checklist.md RELEASING.md docs/dogfood/release-channel.md docs/dogfood/success-metrics.md
git commit -m "docs: close out adoption and release guidance"
```

---

## Wave 4 - Release Hardening and Ship Readiness

### Task 4.1: Run the full contract suite and fix any regressions

**Parallel:** no
**Blocked by:** Task 2.3, Task 3.3
**Owned files:** `CHANGELOG.md`, `README.md`, `docs/project-status.md`

**Files:**
- Modify only if needed after failures: affected source/test/doc files
- Always update if status changed: `CHANGELOG.md`, `docs/project-status.md`

**Step 1: Run the required gates**

Run:

```bash
cargo test
cargo test contract_fixtures
cargo test tier_a_golden
cargo test golden_corpus
```

Expected: all PASS.

**Step 2: If any command fails, fix via TDD**

For each failure:

```text
1. add or update a failing targeted test
2. implement the minimal fix
3. rerun targeted test
4. rerun the failed suite
```

**Step 3: Update status docs**

Record the final verified test commands in `docs/project-status.md`.

**Step 4: Commit**

```bash
git add CHANGELOG.md README.md docs/project-status.md
git commit -m "test: verify final contract suites for release readiness"
```

---

### Task 4.2: Prepare final release notes for the next tag

**Parallel:** no
**Blocked by:** Task 4.1
**Owned files:** `CHANGELOG.md`, `docs/roadmap.md`, `README.md`, `RELEASING.md`

**Files:**
- Modify: `CHANGELOG.md`, `docs/roadmap.md`, `README.md`, `RELEASING.md`

**Step 1: Write the release note outline**

Use this structure in `CHANGELOG.md`:

```md
## [Unreleased]

### Added
### Changed
### Docs
### Operator Notes
```

**Step 2: Ensure every shipped capability is represented once**

Especially:
- envelope AST checks
- policy-diff
- SARIF
- doctor ownership
- monorepo support
- import hygiene / visibility hardening

**Step 3: Add a short operator upgrade note**

Example:

```md
- Repos can now gate widenings either with `specgate policy-diff` in CI or `specgate check --deny-widenings` when a single-command gate is preferred.
```

**Step 4: Verify no release note contradicts docs**

Run:

```bash
rg "deny-widenings|policy-diff|SARIF|ownership|Phase 5" CHANGELOG.md README.md docs/roadmap.md RELEASING.md
```

Expected: consistent wording.

**Step 5: Commit**

```bash
git add CHANGELOG.md docs/roadmap.md README.md RELEASING.md
git commit -m "docs: prepare final release notes and upgrade guidance"
```

---

## Final Verification Checklist

- `README.md` matches `CHANGELOG.md`, `docs/project-status.md`, and `docs/roadmap.md`
- `specgate check --deny-widenings` exists and is documented, or is explicitly removed from near-term scope
- `policy-diff` rename semantics are either implemented safely or explicitly documented as fail-closed
- example CI workflow matches current CLI and docs
- release docs include the real gate commands and promotion criteria
- `cargo test`, `contract_fixtures`, `tier_a_golden`, and `golden_corpus` all pass

---

## Owned Files Validation

Parallel tasks in this plan do not overlap on owned files:

- Task 1.1 owns `README.md`
- Task 1.2 owns `docs/reference/operator-guide.md`
- Task 3.1 owns CI example + CI docs
- Task 3.2 owns release docs + dogfood metrics

All other tasks are sequential by design.

---

## Recommended Execution Order

1. Wave 0 immediately
2. Wave 1 next to restore a coherent project story
3. Wave 2 before tagging the next release
4. Wave 3 before wider rollout
5. Wave 4 only after all feature/documentation deltas are merged
