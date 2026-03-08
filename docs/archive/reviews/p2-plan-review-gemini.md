# P2 Policy Governance Implementation Plan — Review

**Reviewer:** Gemini (Reasoning)
**Date:** 2026-03-08
**Verdict:** ⛔ Requires revisions before implementation. Critical security bypass identified.

## 1. The "Rename Deferral" Bypass Vulnerability
**Criteria:** MVP Scoping / Security
**Severity: CRITICAL**
Section 4.3 states that the MVP will "exclude [rename/copy] pairs from widening/narrowing classification to avoid false delete/add interpretation." This creates a trivial, built-in bypass. A developer can rename a `.spec.yml` file (`mv a.spec.yml a2.spec.yml`) and completely rewrite its rules to open all boundaries in the exact same commit. Because it's an `R*` status, the MVP will exclude it, emit a "structural limitation" warning, and exit `0` (clean). This completely defeats the purpose of the tool. Renames or copies that are not paired and evaluated MUST be treated as widenings (or module deletions, see below) to fail closed.

## 2. Complete Module Deletion is Undefined
**Criteria:** Completeness
**Severity: HIGH**
The plan meticulously covers every field *within* a `SpecFile` (Section 3), but it completely fails to define what happens when an entire `.spec.yml` file is deleted (Git status `D`). Deleting a spec file removes all boundary constraints for that directory—this is the ultimate widening change. It must explicitly trigger a widening classification, but it is missing from the data flow and classification matrices.

## 3. `git ls-tree -r` Scales Terribly on Monorepos
**Criteria:** Git Strategy / Risk Register
**Severity: HIGH**
Task 5 / Section 3.5 proposes running `git ls-tree -r --name-only <base>` and `<head>` to build a file universe for evaluating `boundaries.path` coverage. For a massive enterprise monorepo (e.g., 100,000+ files), executing and parsing the entire tree twice per `policy-diff` run will be incredibly slow and memory-intensive. `specgate` should either use `git ls-files` bounded by the module's directory path, or defer this glob intersection analysis to avoid O(N_repo) scaling. 

## 4. Module Location Contradicts Athena Feedback
**Criteria:** Architecture
**Severity: MEDIUM**
Section 1.1 claims to implement Athena finding #6 by placing the feature in a new top-level module `src/policy/`. However, Athena's actual review explicitly stated the EXACT OPPOSITE: "SpecDiff and change classification belong in `src/rules/` (likely `src/rules/diff.rs`), not a new top-level `src/policy/` module, which artificially splits the domain model." The plan misattributes its architectural decision to Athena while ignoring the actual feedback.

## 5. Git Path Quoting Bugs (Missing Null Termination)
**Criteria:** Git Strategy
**Severity: MEDIUM**
The plan dictates using `git diff --name-status` and `git cat-file --batch`. If a file path contains spaces, unusual characters, or non-ASCII characters, Git will quote and C-escape the path in the output unless `-z` (null-terminated) is used. The plan does not mention using `-z`, which means the `policy::git` parser will fail to parse paths with spaces correctly and crash when piping to `cat-file`.
**Recommendation:** Explicitly mandate `git diff -z --name-status` and `git cat-file --batch -Z`.

## 6. Test Strategy Missing Key Evasion Scenarios
**Criteria:** Test Strategy
**Severity: MEDIUM**
The integration tests in Section 7.3 are good but miss several critical adversarial scenarios targeting the diff engine itself:
1. **The Rename Bypass**: Renaming a file while widening policy.
2. **Pure Deletion**: Deleting a `.spec.yml` file entirely.
3. **Syntax Evasion**: Intentionally breaking the YAML syntax in the `head` ref to bypass checks. (Does it fail open? It must fail closed / exit 2).
4. **Git quoting evasion**: Spec files with spaces or newlines in the filename.

## 7. Constraint Severity "Warning to Error"
**Criteria:** Classification Correctness
**Severity: LOW**
Section 3.3 correctly identifies `error -> warning` as widening and `warning -> error` as narrowing. However, it states that adding a constraint is "structural in MVP". Adding a constraint is definitively narrowing (adding a rule where none existed). While marking it structural is safe (won't fail a CI pipeline strictly checking for widening), it under-reports the narrowing effect. This is acceptable for MVP but should be explicitly noted as a conservative choice.

## 8. Missing Field Matrix Check
**Criteria:** Completeness
**Severity: NITPICK**
`src/spec/types.rs` defines a `package` field on `SpecFile`. The plan marks it as "Structural" in Section 3.1. This is correct. The plan successfully accounts for every field in the structs. 

## Summary
The plan requires a revision to address the **Rename Bypass** and **Module Deletion** loopholes before it can be safely implemented. If a developer can delete or rename a policy file to escape governance, the tool fails its primary objective.
