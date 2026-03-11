# Tier 2 + Tier 3 Buildout Implementation Plan

> **For Claude:** Spawn `task-builder` agents to implement this plan. See dependency graph for parallelization.
>
> **Status note (2026-03-10):** This is a historical implementation plan
> snapshot. The sections for cross-file compensation and config-level
> governance diffing have since shipped. Use `docs/roadmap.md` and
> `docs/reference/policy-diff.md` for current operator-facing truth.

**Goal:** Implement the 8 deferred features from the Tier 2/Tier 3 design spec, covering cross-file compensation, config-level governance diffing, edge classification, baseline v2 metadata, import hygiene rules, provider-side visibility gaps, contradictory glob detection, and rule-family fixture expansion.

**Architecture:** Each feature extends an existing subsystem (policy/, rules/, baseline/, graph/, spec/) with new types, logic modules, and integration tests. Features build on each other in recommended order: early tasks extend `PolicyDiffReport` and config types that later tasks depend on. All new logic lives in dedicated files alongside existing modules to avoid bloating `cli/mod.rs`.

**Tech Stack:** Rust (clap, serde, globset, petgraph, serde_json, serde_yaml, chrono, tempfile for tests)

**Verification baseline (run after every task commit):**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
```

---

## Dependency Graph

```
Task 1 (Compensation)
  └─> Task 2 (Config Diff)
        └─> Task 5 (Import Hygiene)
              └─> Task 6 (Visibility Gaps)
                    └─> Task 7 (Glob Detection)
                          └─> Task 8 (Fixture Expansion)
                                └─> Task 9 (Doc Sweep)

Task 3 (Edge Classification) ──parallel──┐
Task 4 (Baseline v2 Metadata) ─parallel──┘──> Task 5
```

## File Ownership Matrix

| File | T1 | T2 | T3 | T4 | T5 | T6 | T7 | T8 | T9 |
|------|----|----|----|----|----|----|----|----|-----|
| `src/policy/compensate.rs` (new) | W | | | | | | | | |
| `src/policy/types.rs` | W | W | | | | | | | |
| `src/policy/mod.rs` | W | W | | | | | | | |
| `src/policy/render.rs` | W | W | | | | | | | |
| `src/policy/config_diff.rs` (new) | | W | | | | | | | |
| `src/policy/git.rs` | | W | | | | | | | |
| `src/cli/policy_diff.rs` | W | | | | | | | | |
| `src/graph/mod.rs` | | | W | | | | | | |
| `src/rules/hygiene.rs` | | | W | | W | | | | |
| `src/verdict/mod.rs` | | | W | | | W | | | |
| `src/cli/analysis.rs` | | | W | | | | | | |
| `src/baseline/mod.rs` | | | | W | | | | | |
| `src/baseline/audit.rs` (new) | | | | W | | | | | |
| `src/cli/baseline_cmd.rs` | | | | W | | | | | |
| `src/spec/config.rs` | | W | | W | W | | W | | |
| `src/spec/types.rs` | | | | | W | | | | |
| `src/rules/boundary.rs` | | | | | | W | | W | |
| `src/spec/ownership.rs` | | | | | | | W | | |
| `src/spec/glob_analysis.rs` (new) | | | | | | | W | | |
| `src/spec/semantic_conflicts.rs` (new) | | | | | | | W | | |
| `src/cli/doctor/ownership.rs` | | | | | | | W | | |
| `src/cli/doctor/mod.rs` | | | | | W | W | | W | |
| `src/cli/doctor/governance.rs` (new) | | | | | | | | W | |
| `README.md` | | | | | | | | | W |
| `docs/reference/operator-guide.md` | | | | | | | | | W |
| `docs/reference/policy-diff.md` | | | | | | | | | W |

W = writes to this file. Tasks sharing a file MUST be sequential.

Parallel-safe pairs: T3 ∥ T4 (zero file overlap confirmed).

---

## Task 1: Cross-File Compensation in `policy-diff`

**Parallel:** no
**Blocked by:** none
**Creates:** `src/policy/compensate.rs`, `tests/policy_diff_compensation.rs`
**Modifies:** `src/policy/types.rs`, `src/policy/mod.rs`, `src/policy/render.rs`, `src/cli/policy_diff.rs`
**Acceptance tests:** `cargo test --test policy_diff_compensation` — all pass

### Context

Currently `build_policy_diff_report` in `src/policy/mod.rs:28-63` runs: discover specs → classify per-file → summarize. A widening in one spec can't be offset by a narrowing in a connected spec. The design adds a compensation phase between classification and summarization.

Key existing types in `src/policy/types.rs`:
- `FieldChange` (line 34) — has `module`, `spec_path`, `classification`, `field`
- `PolicyDiffReport` (line 64) — has `schema_version`, `base_ref`, `head_ref`, `diffs`, `summary`, `errors`
- `ChangeClassification` (line 8) — `Widening`, `Narrowing`, `Structural`

The module graph is NOT available during policy-diff (it only loads spec snapshots via git). Compensation requires building a lightweight dependency graph from the HEAD specs' `allow_imports_from` fields — not the full file-level resolver graph.

### Step 1: Write failing tests for compensation types

Create `tests/policy_diff_compensation.rs`.

```rust
//! Integration tests for cross-file compensation in policy-diff.

use specgate::policy::types::{
    ChangeClassification, ChangeScope, CompensationCandidate, CompensationResult,
    DependencyEdge, FieldChange, PolicyDiffReport,
};

#[test]
fn compensation_candidate_has_typed_relationship() {
    let widening = FieldChange {
        module: "auth".into(),
        spec_path: "auth/.spec.yml".into(),
        scope: ChangeScope::Boundaries,
        field: "public_api".into(),
        classification: ChangeClassification::Widening,
        before: None,
        after: None,
        detail: "added public_api entry".into(),
    };
    let narrowing = FieldChange {
        module: "api".into(),
        spec_path: "api/.spec.yml".into(),
        scope: ChangeScope::Boundaries,
        field: "public_api".into(),
        classification: ChangeClassification::Narrowing,
        before: None,
        after: None,
        detail: "removed public_api entry".into(),
    };
    let edge = DependencyEdge {
        importer: "api".into(),
        provider: "auth".into(),
    };
    let candidate = CompensationCandidate {
        widening: widening.clone(),
        narrowing: narrowing.clone(),
        relationship: edge,
        result: CompensationResult::Offset,
    };
    assert_eq!(candidate.widening.module, "auth");
    assert_eq!(candidate.narrowing.module, "api");
    assert_eq!(candidate.relationship.importer, "api");
    assert_eq!(candidate.relationship.provider, "auth");
}

#[test]
fn report_has_compensations_and_net_classification() {
    let report = PolicyDiffReport {
        schema_version: "1".into(),
        base_ref: "base".into(),
        head_ref: "HEAD".into(),
        diffs: vec![],
        summary: Default::default(),
        errors: vec![],
        compensations: vec![],
        net_classification: ChangeClassification::Structural,
        config_changes: vec![],
    };
    assert!(report.compensations.is_empty());
    assert_eq!(report.net_classification, ChangeClassification::Structural);
}
```

Run: `cargo test --test policy_diff_compensation -- --no-run 2>&1 | head -30`
Expected: Compilation error — `CompensationCandidate`, `DependencyEdge`, `CompensationResult`, new `PolicyDiffReport` fields don't exist yet.

### Step 2: Add compensation types to `src/policy/types.rs`

After the existing `PolicyDiffErrorEntry` struct (around line 102), add:

```rust
/// A typed dependency edge between two modules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependencyEdge {
    /// Module that imports (has `allow_imports_from` listing the provider).
    pub importer: String,
    /// Module being imported from.
    pub provider: String,
}

/// Result of attempting cross-file compensation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationResult {
    /// Narrowing fully offsets the widening.
    Offset,
    /// Narrowing partially offsets (e.g., different cardinality).
    Partial,
    /// Multiple candidates — fail closed, no compensation applied.
    Ambiguous,
}

/// A candidate pairing of a widening with a narrowing for cross-file compensation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompensationCandidate {
    pub widening: FieldChange,
    pub narrowing: FieldChange,
    pub relationship: DependencyEdge,
    pub result: CompensationResult,
}
```

Add to `PolicyDiffReport` struct:

```rust
/// Cross-file compensation candidates (populated when --cross-file-compensation is active).
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub compensations: Vec<CompensationCandidate>,

/// Net classification after compensation. Always populated (defaults to summary-derived).
pub net_classification: ChangeClassification,

/// Config-level governance changes (populated by config diffing — see Task 2).
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub config_changes: Vec<ConfigFieldChange>,
```

**Important:** `net_classification` is NOT optional — it always reflects the report's classification. Without compensation it mirrors `summary.has_widening`. With compensation it accounts for offset widenings. This ensures consumers always have a single authoritative classification field.

Forward-declare `ConfigFieldChange` as an empty struct for now (Task 2 fills it in):

```rust
/// Placeholder — fully defined in Task 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigFieldChange {
    pub field_path: String,
    pub classification: ChangeClassification,
    pub before: String,
    pub after: String,
}
```

Update all existing construction sites for `PolicyDiffReport` (search `PolicyDiffReport {` in the codebase) to include the new fields with defaults:
- `compensations: Vec::new()`
- `net_classification: if summary.has_widening { ChangeClassification::Widening } else { ChangeClassification::Structural }`
- `config_changes: Vec::new()`

Update `src/policy/mod.rs` re-exports to include all new types.

### Step 3: Run tests to verify types compile

Run: `cargo test --test policy_diff_compensation`
Expected: PASS (types exist, fields are accessible)

### Step 4: Write failing test for compensation logic

Add to `tests/policy_diff_compensation.rs`:

```rust
use specgate::policy::compensate::find_compensation_candidates;

#[test]
fn same_field_connected_modules_produce_offset() {
    // api has allow_imports_from: [auth] — direct dependency
    // widening in auth/public_api + narrowing in api/public_api => Offset
    let widenings = vec![make_field_change("auth", "public_api", ChangeClassification::Widening)];
    let narrowings = vec![make_field_change("api", "public_api", ChangeClassification::Narrowing)];
    let edges = vec![DependencyEdge { importer: "api".into(), provider: "auth".into() }];

    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].result, CompensationResult::Offset);
    assert_eq!(candidates[0].relationship.importer, "api");
    assert_eq!(candidates[0].relationship.provider, "auth");
}

#[test]
fn different_field_family_does_not_compensate() {
    let widenings = vec![make_field_change("auth", "public_api", ChangeClassification::Widening)];
    let narrowings = vec![make_field_change("api", "allow_imports_from", ChangeClassification::Narrowing)];
    let edges = vec![DependencyEdge { importer: "api".into(), provider: "auth".into() }];
    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert!(candidates.is_empty());
}

#[test]
fn unconnected_modules_do_not_compensate() {
    let widenings = vec![make_field_change("auth", "public_api", ChangeClassification::Widening)];
    let narrowings = vec![make_field_change("unrelated", "public_api", ChangeClassification::Narrowing)];
    let edges = vec![]; // no dependency
    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert!(candidates.is_empty());
}

#[test]
fn ambiguous_compensation_fails_closed() {
    // One narrowing, two widenings in the same field family and connected
    let widenings = vec![
        make_field_change("auth", "public_api", ChangeClassification::Widening),
        make_field_change("core", "public_api", ChangeClassification::Widening),
    ];
    let narrowings = vec![make_field_change("api", "public_api", ChangeClassification::Narrowing)];
    let edges = vec![
        DependencyEdge { importer: "api".into(), provider: "auth".into() },
        DependencyEdge { importer: "api".into(), provider: "core".into() },
    ];
    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    // Ambiguous: one narrowing could offset either widening
    assert!(candidates.iter().all(|c| c.result == CompensationResult::Ambiguous));
}

// Helper
fn make_field_change(module: &str, field: &str, classification: ChangeClassification) -> FieldChange {
    FieldChange {
        module: module.into(),
        spec_path: format!("{}/.spec.yml", module),
        scope: ChangeScope::Boundaries,
        field: field.into(),
        classification,
        before: None,
        after: None,
        detail: format!("{:?} in {}/{}", classification, module, field),
    }
}
```

Run: `cargo test --test policy_diff_compensation -- --no-run 2>&1 | head -30`
Expected: Compilation error — `compensate` module doesn't exist.

### Step 5: Create `src/policy/compensate.rs`

```rust
//! Cross-file compensation logic for policy-diff.
//!
//! Scoped compensation: a narrowing in module A can offset a widening in module B
//! only if A and B share a direct dependency relationship, and the changes are in
//! the same field family. Ambiguous cases fail closed.

use std::collections::BTreeSet;

use super::types::{
    ChangeClassification, CompensationCandidate, CompensationResult, DependencyEdge, FieldChange,
};

/// Extract the "field family" from a field name for compensation matching.
/// Same field name = same family.
fn field_family(field: &str) -> &str {
    field
}

/// Check if two modules share a direct dependency edge (either direction).
fn find_edge<'a>(
    module_a: &str,
    module_b: &str,
    edges: &'a [DependencyEdge],
) -> Option<&'a DependencyEdge> {
    edges.iter().find(|e| {
        (e.importer == module_a && e.provider == module_b)
            || (e.importer == module_b && e.provider == module_a)
    })
}

/// Find compensation candidates between widenings and narrowings.
///
/// Rules:
/// - Same field family only
/// - Direct dependency relationship required (typed `DependencyEdge`)
/// - If a narrowing could offset multiple widenings (or vice versa), mark as `Ambiguous`
pub fn find_compensation_candidates(
    widenings: &[FieldChange],
    narrowings: &[FieldChange],
    edges: &[DependencyEdge],
) -> Vec<CompensationCandidate> {
    let mut candidates = Vec::new();

    for narrowing in narrowings {
        debug_assert_eq!(narrowing.classification, ChangeClassification::Narrowing);

        let compatible: Vec<(&FieldChange, &DependencyEdge)> = widenings
            .iter()
            .filter_map(|w| {
                if w.classification != ChangeClassification::Widening {
                    return None;
                }
                if field_family(&w.field) != field_family(&narrowing.field) {
                    return None;
                }
                if w.module == narrowing.module {
                    return None;
                }
                find_edge(&w.module, &narrowing.module, edges).map(|e| (w, e))
            })
            .collect();

        if compatible.len() == 1 {
            let (widening, edge) = compatible[0];
            candidates.push(CompensationCandidate {
                widening: widening.clone(),
                narrowing: narrowing.clone(),
                relationship: edge.clone(),
                result: CompensationResult::Offset,
            });
        } else if compatible.len() > 1 {
            // Ambiguous: one narrowing matches multiple widenings — emit all as Ambiguous
            for (widening, edge) in &compatible {
                candidates.push(CompensationCandidate {
                    widening: (*widening).clone(),
                    narrowing: narrowing.clone(),
                    relationship: (*edge).clone(),
                    result: CompensationResult::Ambiguous,
                });
            }
        }
    }

    // Dedup: if multiple narrowings matched the same widening, mark those as Ambiguous
    let mut widening_keys: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut duplicate_keys: BTreeSet<(&str, &str)> = BTreeSet::new();
    for c in &candidates {
        if c.result == CompensationResult::Offset {
            let key = (c.widening.module.as_str(), c.widening.field.as_str());
            if !widening_keys.insert(key) {
                duplicate_keys.insert(key);
            }
        }
    }
    for c in &mut candidates {
        let key = (c.widening.module.as_str(), c.widening.field.as_str());
        if duplicate_keys.contains(&key) {
            c.result = CompensationResult::Ambiguous;
        }
    }

    candidates
}

/// Extract dependency edges from HEAD specs' `allow_imports_from` fields.
pub fn dependency_edges_from_specs(specs: &[crate::spec::SpecFile]) -> Vec<DependencyEdge> {
    let mut edges = Vec::new();
    for spec in specs {
        if let Some(boundaries) = &spec.boundaries {
            if let Some(allowed) = &boundaries.allow_imports_from {
                for provider in allowed {
                    edges.push(DependencyEdge {
                        importer: spec.module.clone(),
                        provider: provider.clone(),
                    });
                }
            }
        }
    }
    edges
}
```

Register in `src/policy/mod.rs`:
- Add `pub mod compensate;` after existing module declarations
- Re-export: `pub use compensate::{find_compensation_candidates, dependency_edges_from_specs};`

### Step 6: Run compensation tests

Run: `cargo test --test policy_diff_compensation`
Expected: PASS

### Step 7: Wire compensation into the pipeline

In `src/policy/mod.rs`, add:

```rust
#[derive(Debug, Clone, Default)]
pub struct PolicyDiffOptions {
    pub cross_file_compensation: bool,
}

pub fn build_policy_diff_report_with_options(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
    options: &PolicyDiffOptions,
) -> Result<PolicyDiffReport, PolicyGitError> {
    let mut report = build_policy_diff_report(project_root, base_ref, head_ref)?;

    if options.cross_file_compensation {
        apply_compensation(&mut report, project_root, head_ref)?;
    }

    Ok(report)
}
```

The `apply_compensation` function:
1. Loads HEAD specs from git via `git cat-file` (reuse existing blob loading from `git.rs`)
2. Calls `dependency_edges_from_specs` to get typed `DependencyEdge` values
3. Separates widenings and narrowings from all `report.diffs[].changes`
4. Calls `find_compensation_candidates`
5. Stores candidates in `report.compensations`
6. Recomputes `report.net_classification`: if all widenings are offset, classification is `Narrowing` or `Structural`; if any widening remains uncompensated, stays `Widening`

### Step 8: Add `--cross-file-compensation` flag to CLI

In `src/cli/policy_diff.rs`, add to `PolicyDiffArgs`:

```rust
/// Enable cross-file compensation analysis (scoped to directly-connected modules).
#[arg(long, default_value_t = false)]
pub cross_file_compensation: bool,
```

Update `handle_policy_diff` to construct `PolicyDiffOptions` and call `build_policy_diff_report_with_options`.

### Step 9: Update render to show compensated pairs

In `src/policy/render.rs`, add rendering for compensation entries in both human and JSON formats.

Human format:
```
  COMPENSATED: widening in auth/public_api offset by narrowing in api/public_api
               relationship: api imports from auth (direct dependency)
```

JSON format: `"compensations"` array in the report, `"net_classification"` at top level.

NDJSON format: each compensation candidate as a separate line with `"type": "compensation"`.

### Step 10: Write end-to-end integration test

Add a test in `tests/policy_diff_compensation.rs` that creates a temp git repo with two spec files, makes changes that create a widening+narrowing pair between connected modules, runs `build_policy_diff_report_with_options` with compensation enabled, and asserts the report shows `net_classification: Structural`.

### Step 11: Run full verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
```

Expected: PASS

### Step 12: Commit

```bash
git add src/policy/compensate.rs src/policy/types.rs src/policy/mod.rs \
  src/policy/render.rs src/cli/policy_diff.rs tests/policy_diff_compensation.rs
git commit -m "feat: add cross-file compensation to policy-diff

Scoped compensation allows a narrowing in one spec to offset a widening
in a connected spec (via allow_imports_from edges). Same field family only.
Ambiguous cases fail closed. Opt-in via --cross-file-compensation flag."
```

---

## Task 2: Config-Level Governance Diffing

**Parallel:** no
**Blocked by:** Task 1 (uses updated `PolicyDiffReport` types)
**Creates:** `src/policy/config_diff.rs`, `tests/policy_diff_config.rs`
**Modifies:** `src/policy/types.rs` (flesh out `ConfigFieldChange`), `src/policy/git.rs`, `src/policy/mod.rs`, `src/policy/render.rs`, `src/spec/config.rs` (if `Default` needed)
**Acceptance tests:** `cargo test --test policy_diff_config` — all pass

### Context

Currently `discover_spec_file_changes` in `src/policy/git.rs` only looks for `.spec.yml` file changes. Config changes to `specgate.config.yml` (e.g., relaxing `jest_mock_mode` from `enforce` to `warn`) slip through undetected.

The config struct is `SpecConfig` in `src/spec/config.rs`. Task 1 forward-declared `ConfigFieldChange` — this task fills it in and implements classification logic.

### Step 1: Write failing test for config classification logic

Create `tests/policy_diff_config.rs`:

```rust
use specgate::policy::config_diff::classify_config_changes;
use specgate::policy::types::ChangeClassification;
use specgate::spec::config::{JestMockMode, SpecConfig, StaleBaselinePolicy, UnresolvedEdgePolicy};

#[test]
fn jest_mock_mode_enforce_to_warn_is_widening() {
    let mut base = SpecConfig::default();
    base.jest_mock_mode = JestMockMode::Enforce;
    let mut head = SpecConfig::default();
    head.jest_mock_mode = JestMockMode::Warn;
    let changes = classify_config_changes(&base, &head);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].classification, ChangeClassification::Widening);
    assert_eq!(changes[0].field_path, "jest_mock_mode");
}

#[test]
fn jest_mock_mode_warn_to_enforce_is_narrowing() {
    let mut base = SpecConfig::default();
    base.jest_mock_mode = JestMockMode::Warn;
    let mut head = SpecConfig::default();
    head.jest_mock_mode = JestMockMode::Enforce;
    let changes = classify_config_changes(&base, &head);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].classification, ChangeClassification::Narrowing);
}

#[test]
fn added_exclude_pattern_is_widening() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.exclude.push("**/new-exclude/**".into());
    let changes = classify_config_changes(&base, &head);
    let exclude_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "exclude").collect();
    assert_eq!(exclude_changes.len(), 1);
    assert_eq!(exclude_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn removed_exclude_pattern_is_narrowing() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    // Remove one of the default excludes
    head.exclude.retain(|e| e != "**/node_modules/**");
    let changes = classify_config_changes(&base, &head);
    let exclude_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "exclude").collect();
    assert_eq!(exclude_changes.len(), 1);
    assert_eq!(exclude_changes[0].classification, ChangeClassification::Narrowing);
}

#[test]
fn strict_ownership_true_to_false_is_widening() {
    let mut base = SpecConfig::default();
    base.strict_ownership = true;
    let head = SpecConfig::default(); // false by default
    let changes = classify_config_changes(&base, &head);
    let ownership_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "strict_ownership").collect();
    assert_eq!(ownership_changes.len(), 1);
    assert_eq!(ownership_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn unresolved_edge_policy_error_to_ignore_is_widening() {
    let mut base = SpecConfig::default();
    base.unresolved_edge_policy = UnresolvedEdgePolicy::Error;
    let mut head = SpecConfig::default();
    head.unresolved_edge_policy = UnresolvedEdgePolicy::Ignore;
    let changes = classify_config_changes(&base, &head);
    let policy_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "unresolved_edge_policy").collect();
    assert_eq!(policy_changes.len(), 1);
    assert_eq!(policy_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn envelope_enabled_true_to_false_is_widening() {
    let base = SpecConfig::default(); // envelope.enabled defaults to true
    let mut head = SpecConfig::default();
    head.envelope.enabled = false;
    let changes = classify_config_changes(&base, &head);
    let envelope_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "envelope.enabled").collect();
    assert_eq!(envelope_changes.len(), 1);
    assert_eq!(envelope_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn import_hygiene_deny_deep_imports_removed_is_widening() {
    let mut base = SpecConfig::default();
    base.import_hygiene.deny_deep_imports = vec!["lodash".into()];
    let head = SpecConfig::default(); // empty deny list
    let changes = classify_config_changes(&base, &head);
    let hygiene_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "import_hygiene.deny_deep_imports").collect();
    assert_eq!(hygiene_changes.len(), 1);
    assert_eq!(hygiene_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn no_changes_produces_empty() {
    let config = SpecConfig::default();
    let changes = classify_config_changes(&config, &config);
    assert!(changes.is_empty());
}

#[test]
fn telemetry_change_is_structural() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.telemetry = true;
    let changes = classify_config_changes(&base, &head);
    let tel_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "telemetry").collect();
    assert_eq!(tel_changes.len(), 1);
    assert_eq!(tel_changes[0].classification, ChangeClassification::Structural);
}

#[test]
fn spec_dirs_removed_is_widening() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.spec_dirs = vec![]; // removed all spec dirs
    let changes = classify_config_changes(&base, &head);
    let dir_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "spec_dirs").collect();
    assert_eq!(dir_changes.len(), 1);
    assert_eq!(dir_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn stale_baseline_fail_to_warn_is_widening() {
    let mut base = SpecConfig::default();
    base.stale_baseline = StaleBaselinePolicy::Fail;
    let mut head = SpecConfig::default();
    head.stale_baseline = StaleBaselinePolicy::Warn;
    let changes = classify_config_changes(&base, &head);
    let stale_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "stale_baseline").collect();
    assert_eq!(stale_changes.len(), 1);
    assert_eq!(stale_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn escape_hatches_require_expiry_true_to_false_is_widening() {
    let mut base = SpecConfig::default();
    base.escape_hatches.require_expiry = true;
    let head = SpecConfig::default(); // false by default
    let changes = classify_config_changes(&base, &head);
    let eh_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "escape_hatches.require_expiry").collect();
    assert_eq!(eh_changes.len(), 1);
    assert_eq!(eh_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn escape_hatches_max_new_increased_is_widening() {
    let mut base = SpecConfig::default();
    base.escape_hatches.max_new_per_diff = Some(5);
    let mut head = SpecConfig::default();
    head.escape_hatches.max_new_per_diff = Some(10);
    let changes = classify_config_changes(&base, &head);
    let eh_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "escape_hatches.max_new_per_diff").collect();
    assert_eq!(eh_changes.len(), 1);
    assert_eq!(eh_changes[0].classification, ChangeClassification::Widening);
}

#[test]
fn escape_hatches_max_new_removed_is_widening() {
    let mut base = SpecConfig::default();
    base.escape_hatches.max_new_per_diff = Some(5);
    let head = SpecConfig::default(); // None by default
    let changes = classify_config_changes(&base, &head);
    let eh_changes: Vec<_> = changes.iter().filter(|c| c.field_path == "escape_hatches.max_new_per_diff").collect();
    assert_eq!(eh_changes.len(), 1);
    assert_eq!(eh_changes[0].classification, ChangeClassification::Widening);
}
```

Run: `cargo test --test policy_diff_config -- --no-run 2>&1 | head -20`
Expected: Compilation error — `classify_config_changes` doesn't exist.

### Step 2: Create `src/policy/config_diff.rs`

Implement `classify_config_changes(base: &SpecConfig, head: &SpecConfig) -> Vec<ConfigFieldChange>`.

Complete field classification table (from design spec + Codex finding re: `import_hygiene.deny_deep_imports`):

| Field path | Widening direction | Narrowing direction |
|------------|-------------------|-------------------|
| `exclude` | added pattern | removed pattern |
| `spec_dirs` | removed dir | added dir |
| `escape_hatches.max_new_per_diff` | increased / `Some→None` | decreased / `None→Some` |
| `escape_hatches.require_expiry` | `true→false` | `false→true` |
| `jest_mock_mode` | `enforce→warn` | `warn→enforce` |
| `stale_baseline` | `fail→warn` | `warn→fail` |
| `enforce_type_only_imports` | `true→false` | `false→true` |
| `unresolved_edge_policy` | `error→warn→ignore` (relaxing) | `ignore→warn→error` (tightening) |
| `strict_ownership` | `true→false` | `false→true` |
| `import_hygiene.deny_deep_imports` | removed entry | added entry |
| `envelope.enabled` | `true→false` | `false→true` |

Structural (informational only, no widening/narrowing polarity):
- `telemetry`, `release_channel`, `tsconfig_filename`, `test_patterns`, `include_dirs`

Implementation pattern for each field: compare base vs head, skip if identical, emit `ConfigFieldChange` with appropriate classification.

For set-valued fields (`exclude`, `spec_dirs`, `deny_deep_imports`): compute set diff. Added items → one direction, removed items → the other. If both added and removed, emit two changes.

Register: `pub mod config_diff;` in `src/policy/mod.rs`.

### Step 3: Run tests

Run: `cargo test --test policy_diff_config`
Expected: PASS

### Step 4: Wire into the pipeline

In `src/policy/git.rs`, add:

```rust
/// Load and diff config files between base and head refs.
pub fn discover_config_changes(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<ConfigFieldChange>, PolicyGitError> {
    let base_config = load_config_from_ref(project_root, base_ref)?;
    let head_config = load_config_from_ref(project_root, head_ref)?;
    Ok(classify_config_changes(&base_config, &head_config))
}

fn load_config_from_ref(project_root: &Path, git_ref: &str) -> Result<SpecConfig, PolicyGitError> {
    // git cat-file -p {ref}:specgate.config.yml
    // If blob doesn't exist (file not present in that ref), return SpecConfig::default()
    // If blob exists, deserialize via serde_yaml
    todo!()
}
```

In `src/policy/mod.rs`, update `build_policy_diff_report`:
1. Call `discover_config_changes` and attach to `report.config_changes`
2. If any config change has `classification == Widening`, set `report.summary.has_widening = true`
3. Update `report.net_classification` accordingly

Handle edge cases:
- Config added where none existed: diff defaults vs new config; only non-default fields are Structural
- Config deleted: diff old config vs defaults; relaxed fields are Widening
- Config unchanged: empty `config_changes`

### Step 5: Update rendering

In `src/policy/render.rs`, add config change rendering in all output formats.

Human format:
```
Config changes (specgate.config.yml):
  WIDENING: jest_mock_mode: enforce → warn
  NARROWING: strict_ownership: false → true
```

JSON format: `"config_changes"` array in report output.

### Step 6: Run full verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
```

Expected: PASS

### Step 7: Commit

```bash
git add src/policy/config_diff.rs src/policy/types.rs src/policy/mod.rs \
  src/policy/git.rs src/policy/render.rs tests/policy_diff_config.rs
git commit -m "feat: add config-level governance diffing to policy-diff

Diffs specgate.config.yml between base and head refs. Classifies field
changes as widening/narrowing/structural using design-spec polarity table.
Config widenings contribute to overall report classification and exit code."
```

---

## Task 3: Unknown Edge Classification (P6)

**Parallel:** yes (no file overlap with Task 4)
**Blocked by:** none
**Creates:** `tests/edge_classification_integration.rs`
**Modifies:** `src/graph/mod.rs` (add `EdgeType` enum + method), `src/rules/hygiene.rs` (refine existing rule), `src/verdict/mod.rs` (enrich output), `src/cli/analysis.rs` (refine existing `edge.unresolved` handling)
**Acceptance tests:** `cargo test --test edge_classification_integration` — all pass

### Context

**Already exists (do not duplicate):**
- `UnresolvedImportRecord` in `src/graph/mod.rs:116-129` — has `is_external: bool` and `kind: EdgeKind`
- `EdgeClassification` in `src/verdict/mod.rs:60-66` — aggregate counts (`resolved`, `unresolved_literal`, `unresolved_dynamic`, `external`, `type_only`)
- `edge.unresolved` rule in `src/cli/analysis.rs:300-310` — already escalates unresolved imports to `PolicyViolation` based on `unresolved_edge_policy`

**What's new:**
1. A first-class `EdgeType` enum (replacing ad-hoc derivation)
2. Per-edge detail in verdict JSON (not just aggregate counts)
3. Rename rule ID from `"edge.unresolved"` to `"hygiene.unresolved_import"` for consistency with hygiene rule family (keep `"edge.unresolved"` as a deprecated alias)
4. SARIF emission for edge findings with `properties.edgeType`

### Step 1: Write failing tests

Create `tests/edge_classification_integration.rs`:

```rust
use specgate::graph::EdgeType;

#[test]
fn edge_type_enum_exists() {
    let _resolved = EdgeType::Resolved;
    let _literal = EdgeType::UnresolvedLiteral;
    let _dynamic = EdgeType::UnresolvedDynamic;
    let _external = EdgeType::External;
}

#[test]
fn unresolved_import_record_derives_edge_type() {
    // Test that edge_type() method exists and returns correct variants
    // based on is_external and kind fields
}

#[test]
fn verdict_includes_per_edge_detail() {
    // Build a verdict from a graph with mixed edge types
    // Assert the JSON output includes "edges" array with typed entries
}
```

### Step 2: Add `EdgeType` enum to `src/graph/mod.rs`

Add near existing `EdgeKind` enum (do NOT create a separate `types.rs` file — keep types in `mod.rs` where all other graph types live):

```rust
/// Classification of a dependency edge by resolution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    Resolved,
    UnresolvedLiteral,
    UnresolvedDynamic,
    External,
}
```

Add method to `UnresolvedImportRecord`:

```rust
impl UnresolvedImportRecord {
    pub fn edge_type(&self) -> EdgeType {
        if self.is_external {
            EdgeType::External
        } else if self.kind == EdgeKind::DynamicImport {
            EdgeType::UnresolvedDynamic
        } else {
            EdgeType::UnresolvedLiteral
        }
    }
}
```

### Step 3: Enrich verdict with per-edge detail

In `src/verdict/mod.rs`, add to the verdict JSON output:

```rust
/// Per-edge detail entry for verdict output.
#[derive(Debug, Clone, Serialize)]
pub struct VerdictEdge {
    pub from_module: String,
    pub to_module: Option<String>,
    pub edge_type: EdgeType,
    pub import_path: String,
    pub file: String,
    pub line: Option<usize>,
}
```

The verdict builder should populate this from resolved graph edges (all `Resolved`) plus `UnresolvedImportRecord` entries (typed via `.edge_type()`).

### Step 4: Refine rule ID and SARIF

In `src/cli/analysis.rs`, update the existing `edge.unresolved` handling:
- Add `HYGIENE_UNRESOLVED_IMPORT_RULE_ID` (`"hygiene.unresolved_import"`) as the primary rule ID
- Keep `"edge.unresolved"` as a recognized alias for backwards compatibility
- When emitting SARIF, include `properties.edgeType` on each result

In `src/rules/hygiene.rs`, add the constant:
```rust
pub const HYGIENE_UNRESOLVED_IMPORT_RULE_ID: &str = "hygiene.unresolved_import";
```

### Step 5: Run full verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
```

### Step 6: Commit

```bash
git add src/graph/mod.rs src/rules/hygiene.rs src/verdict/mod.rs \
  src/cli/analysis.rs tests/edge_classification_integration.rs
git commit -m "feat: add EdgeType enum, per-edge verdict detail, and hygiene.unresolved_import rule

Enriches verdict JSON with typed per-edge entries. Renames edge.unresolved
to hygiene.unresolved_import (old name kept as alias). SARIF output includes
edgeType property on each finding."
```

---

## Task 4: Baseline v2 Metadata

**Parallel:** yes (independent of Tasks 1-3)
**Blocked by:** none
**Creates:** `src/baseline/audit.rs`, `tests/baseline_metadata.rs`
**Modifies:** `src/baseline/mod.rs`, `src/cli/baseline_cmd.rs`, `src/spec/config.rs`
**Acceptance tests:** `cargo test --test baseline_metadata` — all pass

### Context

`BaselineEntry` in `src/baseline/mod.rs` already has `owner`, `reason`, and `expires_at` fields (all optional). What's missing:

1. `added_at: Option<String>` — auto-populated timestamp
2. `--owner` and `--reason` CLI flags on `baseline` command
3. `baseline.require_metadata` config field
4. `baseline list` subcommand with filtering
5. `baseline audit` subcommand

The current CLI uses flat args in `BaselineArgs` at `src/cli/baseline_cmd.rs` — it has `--output` and `--refresh`. This needs to be migrated to subcommands.

**Chrono is already a dependency** in `Cargo.toml` with features `std`, `serde`, `clock`. Use `chrono::Local::now().format("%Y-%m-%d").to_string()` for `added_at`.

### Step 1: Write failing test for `added_at` field

Create `tests/baseline_metadata.rs`:

```rust
use specgate::baseline::BaselineEntry;

#[test]
fn baseline_entry_has_added_at() {
    let entry = BaselineEntry {
        // ... all existing fields ...
        added_at: Some("2026-03-10".into()),
    };
    assert_eq!(entry.added_at, Some("2026-03-10".to_string()));
}
```

Run: `cargo test --test baseline_metadata -- --no-run`
Expected: Compilation error — `added_at` field doesn't exist.

### Step 2: Add `added_at` to `BaselineEntry`

In `src/baseline/mod.rs`, add after `expires_at`:

```rust
/// Auto-populated date when this baseline entry was created (YYYY-MM-DD).
#[serde(skip_serializing_if = "Option::is_none")]
pub added_at: Option<String>,
```

Fix all existing construction sites (search for `BaselineEntry {` across the codebase) to add `added_at: None`.

When creating new entries (the baseline generation path), auto-populate:
```rust
added_at: Some(chrono::Local::now().format("%Y-%m-%d").to_string()),
```

### Step 3: Add `BaselineConfig` to `src/spec/config.rs`

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
pub struct BaselineConfig {
    /// When true, baseline generation requires --owner and --reason flags.
    #[serde(default)]
    pub require_metadata: bool,
}
```

Add to `SpecConfig`:
```rust
#[serde(default)]
pub baseline: BaselineConfig,
```

### Step 4: Migrate CLI to subcommands

In `src/cli/baseline_cmd.rs`, refactor from flat args to subcommands:

```rust
#[derive(Debug, Clone, clap::Args)]
pub struct BaselineArgs {
    #[command(subcommand)]
    pub command: Option<BaselineCommand>,

    // Keep existing flat args for backwards compatibility.
    // When no subcommand is provided, behavior is identical to today.
    #[command(flatten)]
    pub legacy: BaselineLegacyArgs,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum BaselineCommand {
    /// Generate a baseline from current violations.
    Generate(BaselineGenerateArgs),
    /// List baseline entries with filters.
    List(BaselineListArgs),
    /// Audit baseline health and metadata coverage.
    Audit(BaselineAuditArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct BaselineGenerateArgs {
    /// Output path for the baseline file.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    pub output: String,
    /// Refresh existing baseline (update fingerprints, remove resolved entries).
    #[arg(long)]
    pub refresh: bool,
    /// Owner responsible for these baseline entries.
    #[arg(long)]
    pub owner: Option<String>,
    /// Reason for baselining these violations.
    #[arg(long)]
    pub reason: Option<String>,
    #[command(flatten)]
    pub common: CommonProjectArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct BaselineListArgs {
    /// Filter by owner.
    #[arg(long)]
    pub owner: Option<String>,
    /// Show only expired entries.
    #[arg(long)]
    pub expired: bool,
    /// Show entries expiring within N days.
    #[arg(long)]
    pub expiring_within: Option<u32>,
    /// Group results by field.
    #[arg(long, value_parser = ["owner", "rule"])]
    pub group_by: Option<String>,
    /// Output format.
    #[arg(long, default_value = "human")]
    pub format: String,
    #[command(flatten)]
    pub common: CommonProjectArgs,
}

#[derive(Debug, Clone, clap::Args)]
pub struct BaselineAuditArgs {
    /// Output format.
    #[arg(long, default_value = "human")]
    pub format: String,
    #[command(flatten)]
    pub common: CommonProjectArgs,
}
```

**Backwards compatibility:** When `BaselineArgs.command` is `None`, fall through to legacy behavior (generate with `--output` and `--refresh` flags).

### Step 5: Write failing tests for audit logic

Add to `tests/baseline_metadata.rs`:

```rust
use specgate::baseline::audit::{audit_baseline, AuditReport};

#[test]
fn audit_counts_entries_by_owner() {
    // Build a BaselineFile with 4 entries: 2 team-a, 1 team-b, 1 no-owner
    // Assert report.total_entries == 4, by_owner["team-a"].total == 2, entries_without_owner == 1
}

#[test]
fn audit_detects_expired_entries() {
    // Build entries with expires_at: "2026-01-01" (expired), "2026-04-01" (expiring), "2026-12-31" (active)
    // Assert report.expired == 1, report.expiring_within_30d == 1, report.active == 1
}

#[test]
fn audit_reports_metadata_coverage() {
    // Build entries with partial owner/reason coverage
    // Assert has_owner_count and has_reason_count are correct
}

#[test]
fn require_metadata_rejects_missing_owner() {
    // With baseline.require_metadata: true, calling baseline generate without --owner should error
}
```

### Step 6: Implement `src/baseline/audit.rs`

```rust
use std::collections::BTreeMap;
use super::{BaselineEntry, BaselineFile};

#[derive(Debug, Clone)]
pub struct OwnerStats {
    pub total: usize,
    pub expired: usize,
}

#[derive(Debug, Clone)]
pub struct AuditReport {
    pub total_entries: usize,
    pub by_owner: BTreeMap<String, OwnerStats>,
    pub entries_without_owner: usize,
    pub entries_without_reason: usize,
    pub expired: usize,
    pub expiring_within_30d: usize,
    pub no_expiry: usize,
    pub active: usize,
    pub has_owner_count: usize,
    pub has_reason_count: usize,
}

pub fn audit_baseline(baseline: &BaselineFile, today: &str) -> AuditReport {
    // Use chrono::NaiveDate::parse_from_str for date math
    // today + 30 days for "expiring_within_30d" calculation
    // Group by owner, count expiry states
    todo!()
}
```

Register in `src/baseline/mod.rs`: `pub mod audit;`

### Step 7: Implement list filtering

In `src/cli/baseline_cmd.rs`, add `handle_baseline_list`:
- Load baseline file
- Apply filters: `--owner`, `--expired`, `--expiring-within`
- Group by `--group-by` field
- Render as human table or JSON

### Step 8: Implement audit rendering

In `src/cli/baseline_cmd.rs`, add `handle_baseline_audit`:
- Call `audit_baseline`
- Render summary table (human) or structured JSON
- Exit code: non-zero if `baseline.require_metadata` is true and metadata gaps exist

### Step 9: Run full verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
```

### Step 10: Commit

```bash
git add src/baseline/mod.rs src/baseline/audit.rs src/spec/config.rs \
  src/cli/baseline_cmd.rs tests/baseline_metadata.rs
git commit -m "feat: add baseline v2 metadata with added_at, audit subcommand, and list filtering

Baseline entries gain added_at (auto-populated), and the CLI adds baseline
generate/list/audit subcommands. List supports --owner, --expired,
--expiring-within, --group-by filtering. Audit summarizes health.
baseline.require_metadata config enforces --owner/--reason on generate."
```

---

## Task 5: Import Hygiene Rules (P9)

**Parallel:** no
**Blocked by:** Task 3 (edge classification), Task 4 (config changes to `src/spec/config.rs`)
**Creates:** `tests/hygiene_integration.rs`, `tests/fixtures/hygiene/` (fixture dirs)
**Modifies:** `src/rules/hygiene.rs`, `src/spec/config.rs`, `src/spec/types.rs`, `src/cli/doctor/mod.rs`
**Acceptance tests:** `cargo test --test hygiene_integration` — all pass

### Context

`src/rules/hygiene.rs` already has:
- `HYGIENE_DEEP_THIRD_PARTY_RULE_ID` ("hygiene.deep_third_party_import")
- `HYGIENE_TEST_IN_PRODUCTION_RULE_ID` ("hygiene.test_in_production")
- `parse_package_name` function
- `evaluate_hygiene_rules` function

`ImportHygieneConfig` in `src/spec/config.rs` has `deny_deep_imports: Vec<String>` and `test_boundary: TestBoundaryConfig`.

The design extends these with:
1. Structured `deny_deep_imports` entries with `max_depth` and `pattern`
2. Test boundary `mode` field (`bidirectional`/`production_only`/`off`)
3. Per-module spec overrides on `Boundaries`
4. `boundary.canonical_import_dangling` doctor check

### Step 1: Write failing tests for extended config types

Create `tests/hygiene_integration.rs`:

```rust
use specgate::spec::config::{DenyDeepImportEntry, ImportHygieneConfig, TestBoundaryMode};

#[test]
fn deny_deep_import_entry_parses() {
    let yaml = r#"
import_hygiene:
  deny_deep_imports:
    - pattern: "lodash/**"
      max_depth: 1
    - pattern: "@mui/**"
      max_depth: 2
    - pattern: "*"
      max_depth: 2
"#;
    // Parse and verify structured entries
}

#[test]
fn test_boundary_mode_parses() {
    let yaml = r#"
import_hygiene:
  test_boundary:
    enabled: true
    mode: bidirectional
"#;
    // Parse and verify mode enum
}

#[test]
fn module_level_hygiene_override_parses() {
    let yaml = r#"
version: "2.3"
module: api
boundaries:
  path: "src/api/**"
  import_hygiene:
    deny_deep_imports:
      - pattern: "lodash/**"
        max_depth: 0
      - pattern: "internal-sdk/**"
        allow: true
    test_boundary:
      mode: "off"
"#;
    // Parse spec and verify module-level overrides
}
```

### Step 2: Extend config types

In `src/spec/config.rs`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct DenyDeepImportEntry {
    pub pattern: String,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub severity: Option<crate::spec::Severity>,
}

fn default_max_depth() -> usize { 2 }

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TestBoundaryMode {
    #[default]
    Off,
    ProductionOnly,
    Bidirectional,
}
```

Update `ImportHygieneConfig` to support BOTH legacy format and new structured format:

```rust
pub struct ImportHygieneConfig {
    /// Legacy: simple package name strings. Kept for backwards compat.
    #[serde(default)]
    pub deny_deep_imports: Vec<String>,
    /// New: structured entries with pattern + max_depth.
    /// When both are present, structured entries take precedence for matching patterns.
    #[serde(default)]
    pub deny_deep_import_entries: Vec<DenyDeepImportEntry>,
    #[serde(default)]
    pub test_boundary: TestBoundaryConfig,
}
```

**Migration/precedence rule:** When evaluating, structured entries (`deny_deep_import_entries`) are checked first. For any package not matching a structured entry, fall back to legacy `deny_deep_imports` (which uses the default `max_depth` of 2). This ensures zero breaking changes.

Update `TestBoundaryConfig` to add `mode` and `enabled`:

```rust
pub struct TestBoundaryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: TestBoundaryMode,
    // ... existing fields ...
}
```

### Step 3: Add spec-level import hygiene override

In `src/spec/types.rs`, add to `Boundaries`:

```rust
/// Per-module import hygiene overrides (config defaults apply when None).
#[serde(default, skip_serializing_if = "Option::is_none")]
pub import_hygiene: Option<ModuleImportHygiene>,
```

New types:

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModuleImportHygiene {
    #[serde(default)]
    pub deny_deep_imports: Vec<ModuleDenyDeepImportEntry>,
    #[serde(default)]
    pub test_boundary: Option<ModuleTestBoundaryOverride>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModuleDenyDeepImportEntry {
    pub pattern: String,
    /// Override max_depth for this pattern (None = use config default).
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Set to true to exempt this package from deep import checks.
    #[serde(default)]
    pub allow: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModuleTestBoundaryOverride {
    /// Override mode: "off", "production_only", "bidirectional".
    pub mode: TestBoundaryMode,
}
```

### Step 4: Update `evaluate_hygiene_rules`

In `src/rules/hygiene.rs`, refactor the deep import check:

1. Build effective deny list: merge config `deny_deep_import_entries` + legacy `deny_deep_imports` (converted to entries with default depth)
2. For each module, check if spec-level `import_hygiene.deny_deep_imports` exists
3. Apply merge: spec entries with `allow: true` exempt the pattern; spec entries with `max_depth` override config depth
4. Check each import: parse package name, count depth segments, compare to `max_depth`

Refactor test boundary check:

1. Check `test_boundary.enabled` — if false, skip entirely
2. Get effective mode: spec override wins, then config `test_boundary.mode`
3. `Bidirectional`: flag prod→test AND test→non-public-api-of-other-module
4. `ProductionOnly`: flag only prod→test
5. `Off`: skip
6. Test file importing its OWN module's internals is always allowed

### Step 5: Add `boundary.canonical_import_dangling` doctor check

In `src/cli/doctor/mod.rs` (or new file `src/cli/doctor/canonical.rs`):

- For modules with `enforce_canonical_imports: true`, verify `import_id`/`import_ids` resolve to paths covered by `public_api` globs
- If `import_id` points to a file not in `public_api`, emit doctor finding: `boundary.canonical_import_dangling`
- Severity: `warning`

### Step 6: Write integration tests

Test in `tests/hygiene_integration.rs`:

- Deep import `lodash/internal/deep` with `max_depth: 1` → violation
- Deep import `lodash/fp` with `max_depth: 1` → no violation
- Spec override `allow: true` for a package → suppresses config deny
- Spec override `max_depth: 0` → bans all sub-imports
- Test boundary `bidirectional`: catches cross-module test→non-public-api
- Test boundary `production_only`: catches only prod→test
- Test boundary `off` at spec level → no findings
- Canonical import dangling via doctor

### Step 7: Create test fixtures

Create `tests/fixtures/hygiene/` with minimal TS project fixtures for each scenario.

### Step 8: Run full verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
```

### Step 9: Commit

```bash
git add src/rules/hygiene.rs src/spec/config.rs src/spec/types.rs \
  src/cli/doctor/ tests/hygiene_integration.rs tests/fixtures/hygiene/
git commit -m "feat: extend import hygiene with structured deny entries, bidirectional test boundary, and spec overrides

Deep import checks now support per-pattern max_depth with spec-level
overrides (allow: true exempts, max_depth overrides). Test boundary adds
bidirectional/production_only/off modes. Canonical import dangling
added as doctor check."
```

---

## Task 6: Provider-Side Visibility Model — Gap Completion

**Parallel:** no
**Blocked by:** Task 5 (rule engine changes in hygiene/boundary)
**Creates:** `tests/visibility_gaps_integration.rs`
**Modifies:** `src/rules/boundary.rs`, `src/verdict/mod.rs`, `src/cli/doctor/mod.rs`
**Acceptance tests:** `cargo test --test visibility_gaps_integration` — all pass

### Context

Visibility enforcement already exists in `src/rules/boundary.rs` at `check_provider_side` (line 296+):
- `Visibility::Public` → no violation
- `Visibility::Internal` → emits `boundary.visibility.internal` unless in `friend_modules`
- `Visibility::Private` → always emits `boundary.visibility.private`

`allow_imported_by`, `deny_imported_by`, `friend_modules` are all enforced. The design fills 5 specific gaps.

### Step 1: Write failing tests for namespace inference

Create `tests/visibility_gaps_integration.rs`:

```rust
use specgate::rules::boundary::{namespace_prefix, share_namespace};

#[test]
fn namespace_prefix_extracts_parent() {
    assert_eq!(namespace_prefix("services/auth"), Some("services"));
    assert_eq!(namespace_prefix("services/gateway"), Some("services"));
    assert_eq!(namespace_prefix("core"), None); // root-level
    assert_eq!(namespace_prefix("a/b/c"), Some("a/b")); // nested: parent is a/b
}

#[test]
fn modules_share_namespace() {
    assert!(share_namespace("services/auth", "services/gateway"));
    assert!(!share_namespace("services/auth", "services-v2/auth")); // different prefix
    assert!(!share_namespace("services/auth", "core")); // core has no namespace
    assert!(!share_namespace("core", "utils")); // neither has namespace
}

#[test]
fn internal_root_module_acts_as_private() {
    // A root-level module with visibility: internal should be inaccessible
    // to all modules (since no other module shares its namespace)
    // unless friend_modules is set
}
```

### Step 2: Implement namespace helpers

In `src/rules/boundary.rs`, add public functions:

```rust
pub fn namespace_prefix(module_id: &str) -> Option<&str> {
    let last_slash = module_id.rfind('/')?;
    Some(&module_id[..last_slash])
}

pub fn share_namespace(module_a: &str, module_b: &str) -> bool {
    match (namespace_prefix(module_a), namespace_prefix(module_b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}
```

Update the `internal` visibility check in `check_provider_side`:
- Current: allows if importer is in `friend_modules` (exact match)
- New: allows if `share_namespace(provider, importer)` OR importer is in `friend_modules`
- Root-level modules with `internal`: no namespace matches → effectively private unless friended

### Step 3: Add glob matching for provider-side lists

In `src/rules/boundary.rs`, add:

```rust
fn matches_module_pattern(pattern: &str, module_id: &str) -> bool {
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        globset::Glob::new(pattern)
            .ok()
            .map(|g| g.compile_matcher().is_match(module_id))
            .unwrap_or(false)
    } else {
        pattern == module_id
    }
}
```

Replace all direct string equality checks for `allow_imported_by`, `deny_imported_by`, and `friend_modules` with `matches_module_pattern`.

### Step 4: Enrich violation details with re-export chain

When a visibility violation is detected and the edge is a `ReExport` (via `EdgeKind::ReExport`), walk the chain to find the original provider and include the full chain in the violation message:

```
"C → B (re-export) → A, but B is internal to services/"
```

This requires checking the `EdgeKind` on the graph edge that triggered the violation.

### Step 5: Add visibility metadata to violations

In `src/verdict/mod.rs`, add optional fields to violation entries:

```rust
/// The provider module's visibility setting when this violation was produced.
#[serde(skip_serializing_if = "Option::is_none")]
pub provider_visibility: Option<String>,
/// Why access was granted or denied (e.g., "friend_module", "same_namespace", "allow_imported_by").
#[serde(skip_serializing_if = "Option::is_none")]
pub access_grant_reason: Option<String>,
```

Populate in `check_provider_side` when creating violations.

### Step 6: Add doctor finding for redundant lists

In `src/cli/doctor/mod.rs`, add to ownership or a new visibility consistency check:
- If a module appears in BOTH `allow_imported_by` and `friend_modules` → info finding (redundant)
- If a module appears in BOTH `deny_imported_by` and `friend_modules` → warning finding (contradictory, see Task 7)

### Step 7: Write comprehensive integration tests

Test scenarios in `tests/visibility_gaps_integration.rs`:
- `internal` module in `services/auth` accessible from `services/gateway` (same namespace) — allowed
- `internal` module in `services/auth` blocked from `billing/invoices` — violation
- `internal` root module inaccessible without `friend_modules` — violation
- `allow_imported_by: ["services/*"]` matches `services/auth` but not `services/deep/nested`
- `allow_imported_by: ["services/**"]` matches all depths
- Union semantics: module in both `allow_imported_by` and `friend_modules` — access granted once
- Re-export chain violation includes full chain detail

### Step 8: Run full verification and commit

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
git add src/rules/boundary.rs src/verdict/mod.rs src/cli/doctor/mod.rs \
  tests/visibility_gaps_integration.rs
git commit -m "feat: complete provider-side visibility with namespace inference, glob matching, and chain detail

Internal visibility now uses /‐delimited namespace matching. Provider-side
lists (allow_imported_by, deny_imported_by, friend_modules) support glob
patterns. Re-export chain violations include full path detail. Verdict
includes provider_visibility and access_grant_reason metadata."
```

---

## Task 7: Contradictory Glob Detection in Ownership

**Parallel:** no
**Blocked by:** Task 6 (visibility model completion — semantic conflicts reference visibility)
**Creates:** `src/spec/glob_analysis.rs`, `src/spec/semantic_conflicts.rs`, `tests/ownership_glob_integration.rs`
**Modifies:** `src/spec/ownership.rs` (add analysis calls), `src/cli/doctor/ownership.rs` (render new findings), `src/spec/config.rs` (`strict_ownership_level`)
**Acceptance tests:** `cargo test --test ownership_glob_integration` — all pass

### Context

`src/spec/ownership.rs` has `validate_ownership()` — pure domain logic producing `OwnershipReport`.
`src/cli/doctor/ownership.rs` renders the report. Design adds three analysis tiers.

**Important:** New analysis logic goes in `src/spec/` (domain layer), NOT in `src/cli/doctor/` (CLI layer). The CLI layer only renders. This follows the existing clean separation.

### Step 1: Write failing tests for Tier 1 — structural analysis

Create `tests/ownership_glob_integration.rs`:

```rust
use specgate::spec::glob_analysis::{analyze_glob_structural, StructuralFinding};

#[test]
fn detects_tautological_glob() {
    let globs = vec![
        ("mod-a".into(), "**/*".into()),
        ("mod-b".into(), "src/**/*.ts".into()),
    ];
    let findings = analyze_glob_structural(&globs);
    assert!(findings.iter().any(|f| matches!(f, StructuralFinding::Tautological { .. })));
}

#[test]
fn detects_duplicate_globs() {
    let globs = vec![
        ("mod-a".into(), "src/api/**/*.ts".into()),
        ("mod-b".into(), "src/api/**/*.ts".into()),
    ];
    let findings = analyze_glob_structural(&globs);
    assert!(findings.iter().any(|f| matches!(f, StructuralFinding::Duplicate { .. })));
}

#[test]
fn clean_globs_produce_no_findings() {
    let globs = vec![
        ("mod-a".into(), "src/api/**/*.ts".into()),
        ("mod-b".into(), "src/core/**/*.ts".into()),
    ];
    let findings = analyze_glob_structural(&globs);
    assert!(findings.is_empty());
}
```

### Step 2: Create `src/spec/glob_analysis.rs`

```rust
//! Glob pattern analysis for ownership validation.
//! Tier 1: structural analysis (tautological, duplicate, negation conflicts).
//! Tier 2: subset/superset containment analysis.

use std::path::PathBuf;

/// A (module_id, glob_pattern) pair for analysis.
pub type GlobEntry = (String, String);

#[derive(Debug, Clone)]
pub enum StructuralFinding {
    /// Pattern matches everything — likely unintentional as a module boundary.
    Tautological { module: String, pattern: String },
    /// Two modules have identical glob patterns.
    Duplicate { pattern: String, modules: Vec<String> },
    /// Pattern is logically impossible or empty.
    Invalid { module: String, pattern: String, reason: String },
}

#[derive(Debug, Clone)]
pub enum ContainmentFinding {
    /// Child glob is a strict subset of parent glob.
    StrictSubset {
        child_module: String,
        child_pattern: String,
        parent_module: String,
        parent_pattern: String,
        matched_file_count: usize,
    },
    /// One glob dominates another (90%+ overlap).
    DominantOverlap {
        dominant_module: String,
        dominant_pattern: String,
        submissive_module: String,
        submissive_pattern: String,
        overlap_pct: f64,
    },
}

/// Tier 1: structural analysis of glob patterns in isolation.
pub fn analyze_glob_structural(globs: &[GlobEntry]) -> Vec<StructuralFinding> {
    // - Check for tautological patterns: **, **/* , *
    // - Check for exact duplicates across modules
    // - Check for empty or unparseable patterns
    todo!()
}

/// Tier 2: containment analysis using discovered source files.
pub fn analyze_glob_containment(
    globs: &[GlobEntry],
    source_files: &[PathBuf],
) -> Vec<ContainmentFinding> {
    // For each pair of globs:
    //   1. Try structural containment (fast: if A's pattern prefix contains B's)
    //   2. Fall back to empirical: match both against source_files, compute overlap
    //   3. StrictSubset if child_matches ⊆ parent_matches
    //   4. DominantOverlap if overlap > 90%
    todo!()
}
```

Register in `src/spec/mod.rs`: `pub mod glob_analysis;`

### Step 3: Create `src/spec/semantic_conflicts.rs`

```rust
//! Semantic conflict detection across spec files.
//! Cross-references ownership, visibility, and dependency fields.

use crate::spec::SpecFile;

#[derive(Debug, Clone)]
pub enum SemanticConflict {
    /// Module A is private but module B lists A in allow_imports_from.
    PrivateModuleReferenced {
        private_module: String,
        referencing_module: String,
        field: String,
    },
    /// Module A has deny_imported_by: [B] and friend_modules: [B].
    DeniedButFriended {
        module: String,
        target: String,
    },
    /// Module A lists B in allow_imports_from but B is private and A is not in B's friend_modules.
    UnreachableAllow {
        importer: String,
        provider: String,
    },
    /// A denies B and B denies A.
    CircularDeny {
        module_a: String,
        module_b: String,
    },
    /// Contract match.files glob references paths outside the module's boundaries.path.
    ContractOutsideBoundary {
        module: String,
        contract_id: String,
        contract_glob: String,
        boundary_path: String,
    },
}

pub fn detect_semantic_conflicts(specs: &[SpecFile]) -> Vec<SemanticConflict> {
    // For each pair of specs, check:
    // 1. Private module referenced in another's allow_imports_from
    // 2. deny_imported_by + friend_modules overlap within same spec
    // 3. allow_imports_from references a private module that doesn't friend the importer
    // 4. Mutual deny_imported_by
    // 5. Contract match.files outside boundaries.path
    todo!()
}
```

Register in `src/spec/mod.rs`: `pub mod semantic_conflicts;`

### Step 4: Wire into `validate_ownership` in `src/spec/ownership.rs`

Extend `OwnershipReport` with new fields:

```rust
pub structural_findings: Vec<StructuralFinding>,
pub containment_findings: Vec<ContainmentFinding>,
pub semantic_conflicts: Vec<SemanticConflict>,
```

In `validate_ownership`, after existing checks:
1. Call `analyze_glob_structural` on all spec path globs
2. Call `analyze_glob_containment` with discovered source files
3. Call `detect_semantic_conflicts` with all loaded specs
4. Attach results to the report

### Step 5: Update `src/cli/doctor/ownership.rs` rendering

Add rendering for all new finding types:
- Tier 1 (structural) → ERROR severity
- Tier 2 (containment) → WARNING severity
- Tier 3 (semantic) → WARNING severity

### Step 6: Add `strict_ownership_level` config

In `src/spec/config.rs`:

```rust
/// How strictly doctor ownership gates findings.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StrictOwnershipLevel {
    /// Only Tier 1 errors fail (default when strict_ownership: true).
    #[default]
    Errors,
    /// Tier 1 errors AND Tier 2/3 warnings fail.
    Warnings,
}
```

Add to `SpecConfig`:
```rust
#[serde(default)]
pub strict_ownership_level: StrictOwnershipLevel,
```

Update exit code logic in `src/cli/doctor/ownership.rs`:
- `strict_ownership: false` → always pass
- `strict_ownership: true` + `level: Errors` → fail on Tier 1 findings only
- `strict_ownership: true` + `level: Warnings` → fail on any finding

### Step 7: Write comprehensive tests

Test every finding type with minimal fixtures:
- Tautological glob: `**/*` as boundary path
- Duplicate glob: two specs with same path
- Strict subset: `src/api/orders/**` inside `src/api/**`
- Private module referenced in allow_imports_from
- Denied but friended (same module in deny + friend)
- Unreachable allow (private provider, importer not friended)
- Circular deny
- Contract outside boundary

### Step 8: Run full verification and commit

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
git add src/spec/glob_analysis.rs src/spec/semantic_conflicts.rs \
  src/spec/ownership.rs src/spec/mod.rs src/spec/config.rs \
  src/cli/doctor/ownership.rs tests/ownership_glob_integration.rs
git commit -m "feat: add 3-tier contradictory glob detection to doctor ownership

Tier 1 (errors): tautological, duplicate, and invalid glob patterns.
Tier 2 (warnings): strict subset/superset and dominant overlap detection.
Tier 3 (warnings): semantic conflicts — private module references,
denied-but-friended, unreachable allows, circular denies, contract gaps.
strict_ownership_level config controls warning-level gating."
```

---

## Task 8: Rule-Family Fixture Expansion (C02/C06/C07)

**Parallel:** no
**Blocked by:** Tasks 5-7 (all engine features must be stable)
**Creates:** fixtures in `tests/fixtures/golden/`, `src/cli/doctor/governance.rs`
**Modifies:** `src/rules/boundary.rs` (new rules), `src/cli/doctor/mod.rs` (new subcommand), `tests/tier_a_golden.rs`, `tests/golden_corpus.rs`
**Acceptance tests:** `cargo test --test tier_a_golden` — C02/C06/C07 entries pass; `cargo test --test golden_corpus` — all pass

### Context

This task creates deterministic golden fixtures and implements remaining rules. Depends on visibility model (Task 6) and hygiene (Task 5) being complete.

**Fixture contract format:** `expected/{variant}.verdict.json` (NOT `expected.json`). This matches the existing Tier A harness in `tests/tier_a_golden.rs:89-93`.

### Step 1: C02 — Pattern-Aware Mass Assignment

**Rule:** `boundary.pattern_violation`

Add to `src/rules/boundary.rs`:
```rust
pub const BOUNDARY_PATTERN_VIOLATION_RULE_ID: &str = "boundary.pattern_violation";
```

Logic: For each edge from consumer to provider, if the provider has a contract with `match.pattern`, check if the import target matches the pattern regex. If not, emit violation.

**Important:** This requires checking whether the parser exposes imported symbol names. The existing `ImportRecord` has `specifier` (the import path string). For symbol-level checking, the parser would need to expose named imports. If symbols are not available in the current parser, implement at file-level: flag that the consumer imports from a contract's `match.files` where the contract has a restrictive `pattern`, and the violation notes that pattern enforcement is at file-level granularity.

**Fixture:** `tests/fixtures/golden/c02-mass-assignment/`

```
c02-mass-assignment/
├── specgate.config.yml
├── provider/
│   ├── .spec.yml       (module: provider, contract with pattern: "^get")
│   ├── index.ts         (re-exports getUser and setPassword)
│   └── handlers.ts      (defines getUser and setPassword)
├── consumer/
│   ├── .spec.yml       (module: consumer, allow_imports_from: [provider])
│   └── main.ts         (imports setPassword — violates ^get pattern)
├── tsconfig.json
└── expected/
    └── intro.verdict.json
```

### Step 2: C06 — Category-Level Governance

**Doctor check:** `doctor governance-consistency`

Create `src/cli/doctor/governance.rs`:

```rust
use crate::spec::SpecFile;

pub struct GovernanceFinding {
    pub severity: String,
    pub message: String,
    pub modules: Vec<String>,
}

pub fn check_governance_consistency(specs: &[SpecFile]) -> Vec<GovernanceFinding> {
    // For modules sharing a namespace prefix:
    //   - Flag if one is private but another references it in allow_imports_from
    //   - Flag contradictory visibility within the same namespace
    //   - Flag if sibling modules have incompatible deny/allow lists
    todo!()
}
```

Wire into `src/cli/doctor/mod.rs`:
- Add `GovernanceConsistency` variant to the doctor subcommand enum (alongside `Compare` and `Ownership`)
- Add `handle_doctor_governance_consistency` handler

**Fixture:** `tests/fixtures/golden/c06-duplicate-key/`

```
c06-duplicate-key/
├── specgate.config.yml
├── services/
│   ├── auth/.spec.yml     (module: services/auth, visibility: private)
│   └── gateway/.spec.yml  (module: services/gateway, allow_imports_from: [services/auth])
├── tsconfig.json
└── expected/
    └── intro.verdict.json   (governance-consistency finding about intent mismatch)
```

### Step 3: C07 — Visibility Leak

**Rule:** `boundary.visibility_leak`

Add to `src/rules/boundary.rs`:
```rust
pub const BOUNDARY_VISIBILITY_LEAK_RULE_ID: &str = "boundary.visibility_leak";
```

Logic: For each re-export edge (`EdgeKind::ReExport`), if module A's public_api re-exports from module B where B has stricter visibility than A, and the re-export targets a file NOT in B's `public_api`, emit violation.

**Fixture:** `tests/fixtures/golden/c07-registry-collision/`

```
c07-registry-collision/
├── specgate.config.yml
├── internal/
│   ├── .spec.yml       (module: internal, visibility: internal, public_api: [internal/index.ts])
│   └── index.ts        (re-exports secret from ../private/secret.ts)
├── private/
│   ├── .spec.yml       (module: private-mod, visibility: private, public_api: [private/safe.ts])
│   ├── safe.ts
│   └── secret.ts       (NOT in public_api)
├── tsconfig.json
└── expected/
    └── intro.verdict.json
```

### Step 4: Register in Tier A golden tests

In `tests/tier_a_golden.rs`, add test entries following the existing pattern:

```rust
// In the fixture list:
GoldenFixture { id: "c02-mass-assignment", variants: &["intro"] },
GoldenFixture { id: "c07-registry-collision", variants: &["intro"] },
```

C06 is a doctor check, not a `specgate check` output — it needs a separate test entry that runs `doctor governance-consistency` and compares output.

In `tests/golden_corpus.rs`, add the new fixtures to the corpus list.

### Step 5: Run full verification and commit

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
./scripts/ci/mvp_gate.sh
git add src/rules/boundary.rs src/cli/doctor/governance.rs src/cli/doctor/mod.rs \
  tests/fixtures/golden/c02-mass-assignment/ \
  tests/fixtures/golden/c06-duplicate-key/ \
  tests/fixtures/golden/c07-registry-collision/ \
  tests/tier_a_golden.rs tests/golden_corpus.rs
git commit -m "feat: add C02/C06/C07 golden fixtures graduated to Tier A

C02: boundary.pattern_violation rule + fixture for contract pattern enforcement.
C06: doctor governance-consistency check + fixture for namespace intent mismatch.
C07: boundary.visibility_leak rule + fixture for re-export visibility leak.
All fixtures use expected/{variant}.verdict.json format per Tier A harness."
```

---

## Task 9: Documentation Sweep

**Parallel:** no
**Blocked by:** Tasks 1-8 (all features complete)
**Modifies:** `README.md`, `docs/reference/operator-guide.md`, `docs/reference/policy-diff.md`, `docs/roadmap.md`
**Acceptance tests:** Manual review — all new CLI flags, config fields, rules, and doctor checks are documented

### Context

Tasks 1-8 add significant new surface area:
- CLI flags: `--cross-file-compensation`, `baseline generate/list/audit --owner/--reason`
- Config fields: `baseline.require_metadata`, `strict_ownership_level`, `import_hygiene.deny_deep_import_entries`, `test_boundary.mode`
- Spec fields: `boundaries.import_hygiene`
- Rules: `boundary.pattern_violation`, `boundary.visibility_leak`, `hygiene.unresolved_import`
- Doctor checks: `governance-consistency`, canonical import dangling
- Verdict changes: per-edge detail, `provider_visibility`, `access_grant_reason`, `compensations`, `net_classification`, `config_changes`

All of these need documentation updates.

### Step 1: Update `docs/reference/policy-diff.md`

- Document cross-file compensation: how it works, the `--cross-file-compensation` flag, scoped-to-dependencies behavior, fail-closed semantics
- Document config-level governance diffing: which fields are widening/narrowing/structural, edge cases (config added/deleted)
- Remove deferred items from the "Deferred" table that are now implemented

### Step 2: Update `docs/reference/operator-guide.md`

- Add C02, C06, C07 to the rule family table with descriptions
- Document `boundary.pattern_violation`, `boundary.visibility_leak` rules
- Document `doctor governance-consistency` subcommand
- Update Tier A fixture mapping table
- Document import hygiene configuration (structured entries, test boundary modes, spec overrides)
- Document provider-side visibility enhancements (namespace inference, glob patterns)

### Step 3: Update `README.md`

- Add new features to feature list
- Update CLI usage examples if needed

### Step 4: Update `docs/roadmap.md`

- Move implemented items from "Deferred" to "Completed"
- Update status of remaining items

### Step 5: Commit

```bash
git add README.md docs/reference/operator-guide.md docs/reference/policy-diff.md docs/roadmap.md
git commit -m "docs: update reference docs for tier 2/3 buildout features

Documents cross-file compensation, config governance diffing, edge
classification, baseline v2 metadata, import hygiene, visibility model,
glob detection, and C02/C06/C07 rule families."
```

---

## Summary

| Task | Feature | Creates | Modifies | Blocked by |
|------|---------|---------|----------|------------|
| 1 | Cross-file compensation | `compensate.rs`, test | `types.rs`, `mod.rs`, `render.rs`, `policy_diff.rs` | — |
| 2 | Config governance diffing | `config_diff.rs`, test | `types.rs`, `git.rs`, `mod.rs`, `render.rs` | T1 |
| 3 | Edge classification | test | `graph/mod.rs`, `hygiene.rs`, `verdict/mod.rs`, `analysis.rs` | — |
| 4 | Baseline v2 metadata | `audit.rs`, test | `baseline/mod.rs`, `baseline_cmd.rs`, `config.rs` | — |
| 5 | Import hygiene rules | test, fixtures | `hygiene.rs`, `config.rs`, `types.rs`, `doctor/mod.rs` | T3, T4 |
| 6 | Visibility gap completion | test | `boundary.rs`, `verdict/mod.rs`, `doctor/mod.rs` | T5 |
| 7 | Contradictory glob detection | `glob_analysis.rs`, `semantic_conflicts.rs`, test | `ownership.rs`, `doctor/ownership.rs`, `config.rs` | T6 |
| 8 | Rule-family fixtures | fixtures, `governance.rs` | `boundary.rs`, `doctor/mod.rs`, golden tests | T5-T7 |
| 9 | Documentation sweep | — | `README.md`, operator guide, policy-diff ref, roadmap | T1-T8 |

**Parallel execution opportunities:**
- T3 ∥ T4 (zero file overlap)
- T1 → T2 sequential (both touch `policy/types.rs`)
- T5 → T6 → T7 → T8 → T9 sequential (progressive dependency chain)
