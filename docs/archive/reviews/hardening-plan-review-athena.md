# Athena's Adversarial Review: Hardening Implementation Plan

## 1. Dependency Errors & Merge Conflicts
The plan states that P1.7–P1.11 depend on P1.1–P1.6, but leaves open the possibility of parallel execution for the latter half. If P1.7–P1.11 are executed in parallel, they will all attempt to delete massive chunks from the 3,645-line `src/cli/mod.rs` simultaneously. This will cause catastrophic merge conflicts. P1 must be strictly sequential. Furthermore, extracting private functions into new modules (like `util.rs` or `severity.rs`) means their visibility must be upgraded to `pub(crate)` to remain accessible to `check.rs` and `doctor.rs`. The plan completely ignores visibility modifiers.

## 2. Missing Tasks
- **Visibility Updates:** The plan fails to mention adding `pub(crate)` to newly extracted functions and structs.
- **Test Migration:** Unit tests residing at the bottom of `mod.rs` must be physically moved to the new modules alongside the code they test.
- **CI Configuration:** P2's git diffing feature requires modifying the CI configuration to perform full clones (`fetch-depth: 0`).

## 3. LOC Estimates
- **P1 (~3,000 LOC):** Realistic, as it is primarily moving existing code.
- **P2 (~900 LOC):** Wildly optimistic. Building a robust structural YAML diffing engine that understands `specgate` contract semantics, handles edge cases (renamed files, unparseable commits, missing paths), and classifying the AST diffs will easily exceed 2,000 LOC.
- **P3.1/P3.2 (~400 LOC):** Unrealistic. Scaffolding 12 separate adversarial project structures (each requiring configuration, source files, and expected output JSONs) will take much more than 400 lines of boilerplate.

## 4. Risk Underestimation
Rating the P1 CLI Refactor as "Medium" risk is an underestimation. Moving 3,000 lines out of a central `mod.rs` file will obliterate `git blame` history for the core logic of the application unless handled with extreme care. It will also instantly break any open PRs touching the CLI. The technical risk is medium, but the integration and project history risks are high.

## 5. Sequencing Problems
P4 (SARIF Output) is artificially blocked by P1.12 (the `Renderer` trait). This is a false dependency. `src/verdict/format.rs` already exists and successfully formats JSON, NDJSON, and Human output. SARIF output could be implemented immediately as a new function in `format.rs` in parallel with P1, delivering high-value GitHub integration much sooner.

## 6. P2 Design Flaws (Policy Governance)
- **Module Location:** Policy governance is fundamentally about evaluating rules and contracts. `SpecDiff` and change classification belong in `src/rules/` (likely `src/rules/diff.rs`), not a new top-level `src/policy/` module, which artificially splits the domain model.
- **Git-based Snapshot Fragility:** The `git show <ref>:<path>` approach is extremely brittle. In CI environments like GitHub Actions, repositories are cloned shallowly by default (`fetch-depth: 1`). `git show` will fail instantly. Furthermore, the design does not account for module renames (which will be classified as a widening deletion and a narrowing addition) or YAML syntax errors in the base branch (which will crash the parser).

## 7. P3 Fixture Quality (Adversarial Zoo)
The proposed 12 scenarios are decent but overly focused on JS/TS module resolution semantics. They miss critical real-world agent evasion techniques:
- **Hallucinated Imports:** Agents inventing non-existent library imports to satisfy compiler errors.
- **Symlink Evasion:** Using symlinks to step outside the boundary constraints.
- **Path Traversal:** Using `../../../` in import strings to escape the designated root.
- **Wildcard Re-exports:** `export * from './internal'` leaking bounded types unintentionally.

## 8. P4 SARIF
As noted in Sequencing Problems, the dependency on P1.12's `Renderer` trait is unnecessary. SARIF should be built immediately in `src/verdict/format.rs` to unblock GitHub Code Scanning integration.

## 9. Test Strategy Gaps
The verification strategy "all existing tests pass unchanged" for every P1 task is structurally invalid. Because P1 splits one giant file into smaller domain modules, the `#[cfg(test)]` blocks currently living in `mod.rs` MUST be migrated to the new modules (`types.rs`, `util.rs`, etc.). Leaving them in `mod.rs` will either break the tests (due to lost access to private internal functions) or orphan them from the code they are validating.

## 10. What to Cut
If limited to 3 priorities, I would ship:
1. **P1 (CLI Refactor):** The 3,645-line `mod.rs` is a technical debt emergency that must be resolved.
2. **P3 (Adversarial Zoo):** A security tool is only as good as its adversarial test suite.
3. **P4 (SARIF):** Unlocks GitHub Advanced Security, driving enterprise adoption.

**What to cut:** I would completely cut **P2 (Policy Governance)** and **P5-P9**. P2 represents massive scope creep, is architecturally flawed regarding CI environments and rename detection, and will consume disproportionate resources for edge-case tracking.
