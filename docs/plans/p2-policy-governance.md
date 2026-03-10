# P2 Policy Governance (`specgate policy-diff`) Implementation Plan

> **For Claude:** Spawn `task-builder` agent to implement this plan task-by-task.
>
> **Status note (2026-03-10):** This is the historical MVP implementation plan for `policy-diff`. Current behavior has moved beyond this snapshot: semantic rename/copy pairing and `check --deny-widenings` are now implemented. Use `docs/reference/policy-diff.md` and `docs/roadmap.md` for current operator-facing truth.

**Goal:** Add a new `specgate policy-diff` command that compares `.spec.yml` policy between two git refs and deterministically classifies each change as **widening**, **narrowing**, or **structural**.

**Architecture:** Implement policy diffing as a new `src/policy/` domain (separate from runtime rule evaluation in `src/rules/`) with a field-aware semantic classifier over parsed `SpecFile` structs. `src/cli/policy_diff.rs` becomes the command surface, while `src/policy/git.rs` handles ref/diff/blob retrieval with shallow-clone diagnostics, null-safe parsing, and process-efficient batching.

**Tech Stack:** Rust 2024, clap, serde/serde_json, yaml_serde (`serde_yml`), git subprocesses (`git diff -z --name-status`, `git cat-file --batch -Z`, scoped `git ls-tree -rz`), existing Specgate CLI/error conventions.

---

## 0) Scope, MVP, and Follow-up

## MVP (this build)

- `specgate policy-diff` command with:
  - `--base <ref>` (required)
  - `--head <ref>` (optional, default `HEAD`)
  - `--format human|json|ndjson` (default `human`)
- Field-level widening/narrowing/structural classification for **all `SpecFile` fields**
- **Fail-closed file-operation semantics for governance safety:**
  - any `.spec.yml` deletion (`D`) is widening
  - any `.spec.yml` rename/copy (`R*`/`C*`) is widening-risk in MVP (exit 1)
- Shallow clone detection + graceful error (with explicit CI remediation)
- Git diff + blob loading optimization:
  - first `git diff -z --name-status --find-renames --diff-filter=ACDMRT <base>..<head> -- '*.spec.yml'`
  - then blob retrieval only for changed paths, batched via `git cat-file --batch -Z` (no per-file process spawning)
- Monorepo-safe `boundaries.path` comparison via **scoped file universe discovery** (no full-repo `git ls-tree -r` by default)
- Integration tests using adversarial policy-change fixtures (rename bypass attempts, deletion, malformed YAML, weird filenames)
- Exit code contract:
  - `0` = no widenings (only narrowing/structural/no changes)
  - `1` = one or more widenings detected
  - `2` = runtime/git/parse failure (same runtime error convention as existing CLI)

## Explicitly deferred follow-up

- Semantic rename pairing that can safely downgrade `R*`/`C*` from widening-risk when content is semantically equivalent
- Cross-file compensation analysis ("A widened but B narrowed")
- `specgate check --deny-widenings`
- CODEOWNERS automation/integration (guidance docs only now)
- Config-level governance (`specgate.config.yml` diffing)

---

## 1) Architecture Overview

## 1.1 Module placement decision (Athena finding #6)

**Athena feedback (accurate):** Athena recommended placing diff/classification in `src/rules/` (likely `src/rules/diff.rs`) to keep policy logic together.

**Decision for this plan:** still place this feature in a new top-level module: `src/policy/`, not `src/rules/`.

**Why this plan keeps `src/policy/`:**
- `src/rules/` is runtime code-policy evaluation over dependency graph edges (`specgate check`).
- `policy-diff` is governance over policy artifacts across git history (git IO, ref validation, ref-scoped YAML loading, diff rendering).
- Keeping git/ref plumbing in `src/policy/` avoids coupling VCS concerns into runtime rule evaluation paths.
- This is a deliberate architecture tradeoff, not an Athena-endorsed location choice.

**Boundary:**
- `src/policy/` owns ref loading + semantic diffing + classification model.
- `src/cli/policy_diff.rs` owns argument parsing and output dispatch.
- `src/rules/` remains unchanged for MVP.

## 1.2 Proposed files

- Create: `src/policy/mod.rs`
- Create: `src/policy/types.rs`
- Create: `src/policy/git.rs`
- Create: `src/policy/classify.rs`
- Create: `src/policy/render.rs`
- Create: `src/cli/policy_diff.rs`
- Modify: `src/cli/mod.rs` (new subcommand + dispatch)
- Modify: `src/lib.rs` (`pub mod policy;`)
- Add tests:
  - `src/policy/tests.rs` (unit tests for classifiers + git parser helpers)
  - `src/cli/tests.rs` (CLI contract tests)
  - `tests/policy_diff_integration.rs` (git repo fixture integration)

## 1.3 Data flow

1. CLI parses command args (`policy-diff --base ... --head ... --format ...`).
2. `policy::git` validates git context and refs.
3. `policy::git` runs one scoped path discovery call with null-terminated output:
   - `git diff -z --name-status --find-renames --diff-filter=ACDMRT <base>..<head> -- '*.spec.yml'`
4. File-level status classification runs before field-level parsing:
   - `D` on `.spec.yml` => widening (fail-closed)
   - `R*`/`C*` on `.spec.yml` => widening-risk in MVP (fail-closed)
   - `A` on `.spec.yml` => structural (new policy artifact, not a widening bypass)
   - `M`/`T` => continue to blob load + semantic field comparison
5. `policy::git` batch-loads blob contents for needed `<ref>:<path>` tuples via `git cat-file --batch -Z`, not per-file process spawning.
6. Parse YAML into `SpecFile` at both refs.
7. `policy::classify` performs field-aware semantic comparison for `A`/`M`/`T` records.
8. `policy::render` emits human/json/ndjson.
9. CLI computes exit code from summary (`has_widening`, runtime errors).

---

## 2) Key Type Definitions (Rust)

```rust
// src/policy/types.rs
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeClassification {
    Widening,
    Narrowing,
    Structural,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeScope {
    SpecFile,
    Boundaries,
    Constraint,
    Contract,
    ContractMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldChange {
    pub module: String,
    pub spec_path: String,
    pub scope: ChangeScope,
    pub field: String,                // e.g. "boundaries.allow_imports_from"
    pub classification: ChangeClassification,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub detail: String,               // short deterministic reason
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModulePolicyDiff {
    pub module: String,
    pub spec_path: String,
    pub changes: Vec<FieldChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct PolicyDiffSummary {
    pub modules_changed: usize,
    pub widening_changes: usize,
    pub narrowing_changes: usize,
    pub structural_changes: usize,
    pub has_widening: bool,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyDiffReport {
    pub schema_version: String,       // "1"
    pub base_ref: String,
    pub head_ref: String,
    pub diffs: Vec<ModulePolicyDiff>,
    pub summary: PolicyDiffSummary,
    pub errors: Vec<PolicyDiffErrorEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyDiffErrorEntry {
    pub code: String,                 // e.g. "git.shallow_clone_missing_ref"
    pub message: String,
    pub spec_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDiffExit {
    Clean,       // 0
    Widening,    // 1
    RuntimeError // 2
}
```

```rust
// src/cli/policy_diff.rs
#[derive(Debug, Clone, clap::Args)]
pub struct PolicyDiffArgs {
    #[arg(long, default_value = ".")]
    pub project_root: std::path::PathBuf,

    #[arg(long)]
    pub base: String,

    #[arg(long, default_value = "HEAD")]
    pub head: String,

    #[arg(long, value_enum, default_value_t = PolicyDiffFormat::Human)]
    pub format: PolicyDiffFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum PolicyDiffFormat {
    Human,
    Json,
    Ndjson,
}
```

---

## 3) YAML Structural Diff Semantics (mandatory detail)

All comparisons are on parsed structs (`SpecFile`, `Boundaries`, `Constraint`, `BoundaryContract`) and git status metadata, never raw text.

Normalization before comparison:
- Trim strings for set-like fields
- Dedupe + sort list fields into `BTreeSet<String>` when order is non-semantic
- Compare `visibility` via defaulted value (`None => Public`)
- Compare `allow_imports_from` and `allow_imported_by` with tri-state semantics (restricted vs unrestricted)

## 3.0 File-level semantics (`git diff --name-status`)

These rules execute first and are **fail-closed** for governance:

| Status | Meaning | MVP classification | Rationale |
|---|---|---|---|
| `A` | New `.spec.yml` | Structural | New module governance is not a widening bypass |
| `M`/`T` | In-place modified `.spec.yml` | Delegated to field-level matrices | Standard semantic diff |
| `D` | Deleted `.spec.yml` | **Widening** | Removing a policy file removes all constraints for that module |
| `R*` / `C*` | Renamed/copied `.spec.yml` | **Widening (widening-risk) in MVP** | Prevent rename/copy bypass until semantic pairing is implemented |

Notes:
- Rename/copy is intentionally strict in MVP; governance must not be bypassable by moving policy files.
- Future rename pairing may allow a non-widening result when old/new policies are semantically equivalent.

## 3.1 Field matrix for `SpecFile`

| Field | Rule |
|---|---|
| `version` | Structural (metadata/versioning change only in MVP) |
| `module` | Structural (module id rename/reorg marker) |
| `package` | Structural |
| `import_id` | Structural |
| `import_ids` | Structural |
| `description` | Structural |
| `boundaries` | Delegated to boundaries matrix below |
| `constraints` | Constraint matrix below |
| `spec_path` | Ignored (not serialized policy field) |
| file presence (`A`/`D`/`R`/`C`) | Handled by file-level semantics in 3.0 |

## 3.2 Field matrix for `Boundaries`

| Field | Widening | Narrowing | Structural |
|---|---|---|---|
| `path` | New/broader coverage (see 3.5) | Narrower coverage | Equal coverage / ambiguous |
| `public_api` | Added exports | Removed exports | reorder only |
| `allow_imports_from` (`None` = unrestricted) | `Some`→`None`, set additions | `None`→`Some`, set removals | reorder only |
| `never_imports` | removals | additions | reorder only |
| `allow_type_imports_from` | additions | removals | reorder only |
| `visibility` (default Public) | toward `Public` | toward `Private` | none |
| `allow_imported_by` (empty = unrestricted) | restricted→unrestricted, set additions | unrestricted→restricted, set removals | reorder only |
| `deny_imported_by` | removals | additions | reorder only |
| `friend_modules` | additions | removals | reorder only |
| `enforce_canonical_imports` | `true→false` | `false→true` | none |
| `allowed_dependencies` | additions | removals | reorder only |
| `forbidden_dependencies` | removals | additions | reorder only |
| `enforce_in_tests` | `true→false` | `false→true` | none |
| `contracts` | contract removals | contract additions | reorder only |

## 3.3 Constraint semantics (`Vec<Constraint>`)

MVP conservative-but-complete behavior:
- Match constraints by key: `(rule, canonical_json(params))`
- `severity` change:
  - `error -> warning` = widening
  - `warning -> error` = narrowing
- `message` change = structural
- Added/removed constraints = structural by default in MVP.

This is a conservative under-reporting choice: adding constraints is usually narrowing, but MVP keeps add/remove structural until rule-specific semantics are implemented.

## 3.4 Contract semantics (`Vec<BoundaryContract>`)

Match contracts by `id`.

- Contract removed: widening
- Contract added: narrowing
- For matched contract IDs:
  - `envelope: required -> optional` = widening
  - `envelope: optional -> required` = narrowing
  - `match.files` additions = narrowing (broader enforcement coverage)
  - `match.files` removals = widening
  - `match.pattern` `None -> Some` = widening (more selective matching)
  - `match.pattern` `Some -> None` = narrowing
  - `contract` path string change = structural
  - `direction` change = structural in MVP
  - `imports_contract` add/remove = structural in MVP

## 3.5 `boundaries.path` coverage algorithm (monorepo-safe)

Because `path` is semantic glob ownership, classification uses a scoped file-universe coverage check, not a whole-repo tree walk.

1. Derive candidate path prefixes from base/head `boundaries.path` globs for the module under comparison:
   - extract static prefix before first wildcard (`*`, `?`, `[`)
   - if empty prefix, fall back to module directory inferred from spec path
2. Build prefix union and de-duplicate.
3. Query tracked files for each ref only under those prefixes:
   - `git ls-tree -rz --name-only <base> -- <prefix...>`
   - `git ls-tree -rz --name-only <head> -- <prefix...>`
4. Filter to source-like files (`.ts,.tsx,.js,.jsx,.mts,.cts`) plus `.spec.yml` for ownership edges.
5. Compile base/head globs with `globset`.
6. Evaluate matched set cardinality and strict subset/superset relationship.

Classification:
- `head ⊃ base` => widening
- `head ⊂ base` => narrowing
- equal => structural
- invalid glob OR unbounded prefix set => structural + limitation entry (`path_coverage_unbounded_mvp`)

This keeps default behavior scalable for monorepos by avoiding O(N_repo) `git ls-tree -r` scans.

---

## 4) Git/CI behavior (mandatory critiques)

## 4.1 Shallow clone detection + graceful error

At command start:
- `git rev-parse --is-inside-work-tree` must be true
- `git rev-parse --is-shallow-repository`
- `git cat-file -e <base>^{commit}` and `git cat-file -e <head>^{commit}`

If ref object missing and repository is shallow:
- return runtime error (`exit 2`) with explicit guidance:
  - GitHub Actions: `actions/checkout@v4` with `fetch-depth: 0`
  - or run `git fetch --deepen=200 origin <base_ref>`
- include error code: `git.shallow_clone_missing_ref`

If ref missing but repo not shallow:
- return `git.invalid_ref` runtime error.

## 4.2 Git process optimization + null-safe parsing

MVP implementation requirement:
- **Single scoped diff call first (NUL-terminated):**
  - `git diff -z --name-status --find-renames --diff-filter=ACDMRT <base>..<head> -- '*.spec.yml'`
- Parse status/path tuples from NUL-delimited output; do not rely on shell-style quoting.
- Then only changed spec paths are loaded from refs.
- Blob retrieval is batched via `git cat-file --batch -Z` (one process per ref), not N `git show` calls.
- Path-universe discovery for `boundaries.path` uses scoped `git ls-tree -rz --name-only` calls only.

## 4.3 Rename/copy handling (fail-closed in MVP)

MVP does not attempt semantic rename pairing.

Behavior in MVP:
- detect rename/copy statuses from `--name-status --find-renames`
- classify any `.spec.yml` rename/copy (`R*`/`C*`) as widening-risk (widening)
- include explicit detail in report, e.g.:
  - `"rename/copy of policy file modules/foo.spec.yml -> modules/bar.spec.yml treated as widening-risk in MVP"`

Rationale: governance cannot be bypassed by packaging a widening inside a rename/copy.

---

## 5) CLI UX and Output Contracts

## 5.1 Human format (default)

Example skeleton:

```text
Policy diff: base=origin/main head=HEAD

WIDENING (3)
  - module=api/orders field=boundaries.allow_imports_from detail=added ["shared/db"]
  - module=api/orders field=boundaries.visibility detail=private -> internal
  - module=core/auth field=spec_file detail=deletion of modules/core/auth.spec.yml

NARROWING (1)
  - module=ui/checkout field=boundaries.never_imports detail=added ["api/internal"]

STRUCTURAL (2)
  - module=core/auth field=description detail=text changed
  - module=core/auth field=constraints detail=constraint message changed

Summary: modules_changed=2 widening=3 narrowing=1 structural=2
```

## 5.2 JSON format

Single object (`PolicyDiffReport`) with deterministic sort order:
- `diffs` sorted by `(module, spec_path)`
- `changes` sorted by `(classification_rank, field, detail)`
- `errors` sorted by `(code, spec_path, message)`

## 5.3 NDJSON format

Streaming lines for large diffs:
- one line per field change
- final summary line

Proposed event envelope:

```json
{"type":"change","module":"api/orders","field":"boundaries.allow_imports_from","classification":"widening",...}
{"type":"change","module":"ui/checkout","field":"boundaries.never_imports","classification":"narrowing",...}
{"type":"summary","modules_changed":2,"widening_changes":1,"narrowing_changes":1,"structural_changes":0,"has_widening":true}
```

---

## 6) Task Breakdown (with dependencies, LOC, parallelization)

> Realistic estimate (addresses review critique): **~2,700–3,300 LOC total** (implementation + tests + fixtures + docs). This is intentionally above the prior 900 LOC estimate.

### Task 1: Policy domain scaffolding + core types

**Parallel:** no  
**Blocked by:** none  
**Owned files:** `src/policy/mod.rs`, `src/policy/types.rs`, `src/lib.rs`

**Files:**
- Create: `src/policy/mod.rs`
- Create: `src/policy/types.rs`
- Modify: `src/lib.rs`
- Test: `src/policy/tests.rs` (initial type serialization tests)

**Estimate:** 180–260 LOC

Deliverables:
- type model (`PolicyDiffReport`, `FieldChange`, enums)
- deterministic sort helpers
- schema version constants

---

### Task 2: Git ref validation, shallow clone handling, and change discovery

**Parallel:** no  
**Blocked by:** Task 1  
**Owned files:** `src/policy/git.rs`, `src/policy/mod.rs`, `src/policy/tests.rs`

**Files:**
- Create: `src/policy/git.rs`
- Modify: `src/policy/mod.rs`
- Modify: `src/policy/tests.rs`

**Estimate:** 320–460 LOC

Deliverables:
- git repo/ref validators
- shallow clone diagnostics
- NUL-safe parser for `git diff -z --name-status --find-renames --diff-filter=ACDMRT ... '*.spec.yml'`
- changed path set + file-operation classification (`A/M/T` vs `D` vs `R/C` fail-closed)

---

### Task 3: Batched blob loader + parsed snapshot builder

**Parallel:** no  
**Blocked by:** Task 2  
**Owned files:** `src/policy/git.rs`, `src/policy/mod.rs`, `src/policy/tests.rs`

**Files:**
- Modify: `src/policy/git.rs`
- Modify: `src/policy/mod.rs`
- Modify: `src/policy/tests.rs`

**Estimate:** 280–420 LOC

Deliverables:
- `git cat-file --batch -Z` based blob retrieval for `<ref>:<path>` tuples
- YAML parse to `SpecFile` for base/head snapshots
- per-file parse error collection (`PolicyDiffErrorEntry`)

---

### Task 4: Field-level semantic classifier (SpecFile + Boundaries + Constraints + Contracts)

**Parallel:** no  
**Blocked by:** Task 3  
**Owned files:** `src/policy/classify.rs`, `src/policy/mod.rs`, `src/policy/tests.rs`

**Files:**
- Create: `src/policy/classify.rs`
- Modify: `src/policy/mod.rs`
- Modify: `src/policy/tests.rs`

**Estimate:** 700–980 LOC

Deliverables:
- full field matrix implementation (Section 3)
- tri-state allowlist semantics
- visibility ordering semantics
- contract-level envelope + match diffs
- deterministic change sorting

---

### Task 5: `boundaries.path` coverage comparator

**Parallel:** no  
**Blocked by:** Task 4  
**Owned files:** `src/policy/classify.rs`, `src/policy/git.rs`, `src/policy/tests.rs`

**Files:**
- Modify: `src/policy/classify.rs`
- Modify: `src/policy/git.rs`
- Modify: `src/policy/tests.rs`

**Estimate:** 220–340 LOC

Deliverables:
- prefix derivation from compared path globs
- scoped file-universe retrieval via `git ls-tree -rz --name-only <ref> -- <prefix...>`
- glob coverage superset/subset comparison
- unbounded-glob ambiguity fallback behavior (structural + limitation)

---

### Task 6: Renderers (human/json/ndjson)

**Parallel:** yes  
**Blocked by:** Task 4  
**Owned files:** `src/policy/render.rs`, `src/policy/mod.rs`, `src/policy/tests.rs`

**Files:**
- Create: `src/policy/render.rs`
- Modify: `src/policy/mod.rs`
- Modify: `src/policy/tests.rs`

**Estimate:** 180–260 LOC

Deliverables:
- human formatter
- json serializer wrapper (deterministic)
- ndjson event stream formatter

---

### Task 7: CLI command wiring (`specgate policy-diff`)

**Parallel:** yes  
**Blocked by:** Tasks 4, 6  
**Owned files:** `src/cli/policy_diff.rs`, `src/cli/mod.rs`, `src/cli/tests.rs`

**Files:**
- Create: `src/cli/policy_diff.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/tests.rs`

**Estimate:** 220–320 LOC

Deliverables:
- new `Command::PolicyDiff(PolicyDiffArgs)`
- dispatch to handler
- format selection and exit code mapping (`0/1/2`)
- command-level tests for basic contracts

---

### Task 8: Integration tests with adversarial git fixtures

**Parallel:** no  
**Blocked by:** Tasks 5, 7  
**Owned files:** `tests/policy_diff_integration.rs`, `tests/fixtures/policy_diff/**`

**Files:**
- Create: `tests/policy_diff_integration.rs`
- Create: `tests/fixtures/policy_diff/<scenario>/*`

**Estimate:** 420–650 LOC

Deliverables:
- temp git repo setup helper (init/commit/tag)
- fixture scenarios for widening/narrowing/structural/mixed
- shallow clone simulation test (expected graceful error)
- rename + widening bypass attempt test (must fail closed as widening)
- pure spec deletion test (must classify widening)
- malformed YAML in base/head tests (must exit 2, no panic)
- weird filename tests (spaces/unicode/special chars) with NUL-safe git parsing

---

### Task 9: Documentation + CI guidance

**Parallel:** yes  
**Blocked by:** Task 7  
**Owned files:** `README.md`, `docs/reference/policy-diff.md`, `CHANGELOG.md`

**Files:**
- Create: `docs/reference/policy-diff.md`
- Modify: `README.md`
- Modify: `CHANGELOG.md`

**Estimate:** 120–180 LOC

Deliverables:
- command examples
- GitHub Actions `fetch-depth: 0` guidance
- explicit MVP behavior: rename/copy and deletion are fail-closed widenings
- deferred items: semantic rename pairing, cross-file compensation, config-level governance

---

## 7) Test Strategy

## 7.1 Unit tests (`src/policy/tests.rs`)

- Classification matrix tests for each field direction:
  - `allow_imports_from` tri-state transitions
  - `never_imports` add/remove
  - `public_api` add/remove
  - `visibility` partial-order transitions
  - `envelope` required/optional transitions
  - `max_new_per_diff` is config-level (explicitly **not** in MVP command output)
- File-operation semantics tests:
  - `.spec.yml` deletion (`D`) => widening
  - `.spec.yml` rename/copy (`R*`/`C*`) => widening-risk widening
- Constraint severity change tests (`error<->warning`)
- Constraint add/remove marked structural in MVP (with explicit conservative-note assertion)
- Contract add/remove and contract-match pattern/files tests
- Null-terminated git parser tests for paths with spaces/unicode/escaped characters
- Deterministic sort tests
- NDJSON summary line tests

## 7.2 CLI tests (`src/cli/tests.rs`)

- `policy-diff` appears in command parsing
- `--format` values accepted/rejected correctly
- exit code contract:
  - widening => `1`
  - only narrowing/structural => `0`
  - git/runtime failure => `2`

## 7.3 Integration tests (`tests/policy_diff_integration.rs`)

Scenarios (adversarial emphasis):
1. `widen_allow_imports_from`
2. `remove_never_imports`
3. `public_api_addition`
4. `visibility_private_to_public`
5. `contract_envelope_required_to_optional`
6. `narrowing_only_change_set`
7. `structural_only_reorder_comments` (same semantics)
8. `mixed_widen_narrow_structural`
9. `rename_with_widening_attempt_fail_closed` (R-status cannot bypass governance)
10. `pure_spec_deletion_is_widening`
11. `shallow_clone_missing_base_ref` (graceful guidance)
12. `invalid_yaml_in_base_ref_exit_2` (runtime error entry, no panic)
13. `invalid_yaml_in_head_ref_exit_2` (runtime error entry, no panic)
14. `path_glob_broadened` and `path_glob_narrowed`
15. `spec_filename_with_spaces_and_unicode` (NUL-safe parsing proof)

Assertions:
- deterministic summary counts
- correct classification labels
- rename/copy and deletion paths produce widening
- malformed YAML fails closed with exit `2` and no panic
- stable JSON shape for CI consumption

---

## 8) Risk Register

| Risk | Impact | Likelihood | Mitigation |
|---|---:|---:|---|
| Shallow clone base/head not available | High | High (CI default) | explicit shallow detection + actionable error text + doc guidance |
| Rename/copy used as governance bypass | High | Medium | classify `R*`/`C*` on `.spec.yml` as widening-risk in MVP (fail-closed) |
| Spec file deletion silently removes governance | High | Medium | classify `.spec.yml` `D` as widening |
| Over/under-classifying path glob changes | Medium | Medium | scoped prefix coverage comparison + unbounded fallback to structural limitation |
| Full-repo tree walk performance on monorepos | Medium | Medium | avoid global `git ls-tree -r`; use scoped `ls-tree -rz` by candidate prefixes |
| Constraint semantics too conservative | Medium | Medium | document that add/remove constraints are structural in MVP and under-report narrowing; follow-up rule-aware classifier |
| Parse failures in historical ref | Medium | Medium | collect per-file errors, return runtime code 2, never panic |
| Git path quoting/escaping bugs | High | Low | require `-z`/`-Z` and test weird filenames |
| Non-deterministic ordering | High | Low | explicit stable sorts on modules/fields/errors/events |

---

## 9) Checkpoint Assertions (measurable gates)

### Checkpoint A (after Task 3)
- `src/policy/` exists with `mod.rs`, `types.rs`, `git.rs`
- batched blob retrieval implemented (`git cat-file --batch -Z`, no loop of per-file `git show` spawns)
- tests proving shallow clone error code/message path pass

### Checkpoint B (after Task 5)
- full classifier exists for all `SpecFile` fields + file-operation semantics
- `boundaries.path` superset/subset logic covered by tests using scoped universe retrieval
- unit test count for `src/policy/tests.rs` >= 25

### Checkpoint C (after Task 7)
- `specgate policy-diff --help` shows base/head/format flags
- exit codes match contract (`0/1/2`) in CLI tests
- JSON output includes `schema_version`, `summary`, `diffs`, `errors`

### Checkpoint D (after Task 8)
- `tests/policy_diff_integration.rs` green with >= 12 scenarios
- includes explicit rename bypass attempt, pure deletion, malformed YAML (base/head), weird filename cases
- deterministic snapshot assertions pass in repeated runs

### Final Gate
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- Optional: add `cargo test policy_diff -- --nocapture` smoke target for local debugging

---

## 10) CI usage contract

Primary CI invocation:

```bash
specgate policy-diff --base origin/main
```

Explicit SHA comparison:

```bash
specgate policy-diff --base <sha> --head <sha>
```

GitHub Actions requirement (for non-shallow history):

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
```

Interpretation:
- Exit `0`: safe from widening perspective
- Exit `1`: widening detected (policy governance failure)
- Exit `2`: tool/runtime/git failure (pipeline should fail as infra/tooling issue)

---

## 11) Notes on deferred items

- **`max_new_per_diff` widening detection** belongs to config diffing (`specgate.config.yml`), not `.spec.yml`; deferred by design in this MVP per requested scope boundaries.
- **Cross-file compensation analysis** intentionally deferred to avoid false trust in net-zero arithmetic without domain constraints.
- **Semantic rename pairing** is deferred, but MVP remains fail-closed by classifying rename/copy as widening-risk.
- **`check --deny-widenings`** should call into the same `policy::diff` API in follow-up; no duplicate logic.
