# Specgate Implementation Plan

**Version:** 1.1 — February 25, 2026  
**Purpose:** Definitive Rust build guide for coding agents (MVP-focused)  
**Language:** Rust  
**MVP Scope:** File-edge structural policy engine with deterministic output contract

---

## Changelog (v1.1)

- Added deterministic output contract split: `deterministic` (default, byte-identical) vs optional `metrics` mode.
- Added provider-side visibility controls and precedence with importer-side rules.
- Added canonical module import identifiers + optional cross-module canonical import enforcement.
- Added baseline workflow: `specgate baseline` with violation fingerprints and CI handling.
- Added governance output for spec-collusion prevention (`spec_files_changed`, `rule_deltas`).
- Expanded parser/extractor edges to include `require("literal")`, `import("literal")`, and configurable `jest.mock` handling.
- Added resolver parity tooling: `specgate doctor compare` against `tsc --traceResolution`.
- Trimmed MVP risk: keep file-edge checks; defer deep symbol-origin tracing to Phase 1.5 unless rule-required.
- Kept Rust implementation direction (no TS migration).

---

## Build Execution Status Refresh (2026-02-25)

This section updates the plan against actual repository history and test results.

### Milestone Completion (verified)

- [x] **Wave 0 contract lock merged**
  - `aa918ad` — `feat(wave0): implement contract lock for CLI semantics, version policy, and tests`
  - `de192c5` — merge to `master`
  - Contract surface documented in `WAVE0_CONTRACT.md`; fixtures in `tests/contract_fixtures.rs`.
- [x] **Golden v1 scaffold merged**
  - `2e52949` — `feat(golden-corpus): add top-5 golden corpus fixtures`
  - `0d06bf4` — merge to `master` (tagged `specgate-wave0-golden-v1-2026-02-25`)
  - Coverage in `tests/golden_corpus.rs`.
- [x] **Tier A P0 implemented**
  - `0297381` — `test: add Tier A fixtures and strict deterministic gate`
  - Gate implemented in `tests/tier_a_golden.rs`.
- [x] **Reviewer hardening pass completed**
  - `7a7fab8` — `test(tier-a): harden A03/A06 near-miss contracts and null to_module`
  - Added precision checks for near-miss variants and explicit `to_module: null` assertion for A06 cycle contract.

### Current verification snapshot

- `cargo test`: **PASS** on `master`
  - 123 unit + 12 contract fixtures + 10 golden corpus + 28 integration + 1 Tier A + 8 wave2c CLI.
- Contract/docs alignment: Wave 0 semantics represented in `WAVE0_CONTRACT.md` and covered by tests.

### MVP completion estimate

- **~80% complete for MVP hard gate readiness**.
- Core engine + contract-critical semantics are in place; remaining work is mostly productization hardening, CI wiring, and trust/governance polish.

---

## 1. Project Setup (unchanged direction)

- Initialize Rust repo (`cargo init --name specgate`)
- Edition 2024, MSRV 1.85+
- Single-crate MVP with module boundaries for later split

High-level source layout:

```
src/
  cli/         # commands
  spec/        # schema + config + loading
  resolver/    # oxc_resolver wrapper + doctor parity helpers
  parser/      # oxc_parser traversal + import/dependency extraction
  graph/       # global file dependency graph
  rules/       # boundary/dependency/layer/cycle evaluation
  baseline/    # fingerprint generation + baseline IO
  verdict/     # deterministic + metrics output builders
```

---

## 2. Core Schema Updates (v2.2 compatibility)

### `SpecFile` additions

```rust
pub struct SpecFile {
    pub version: String,                 // "2.2"
    pub module: String,
    pub package: Option<String>,
    pub import_id: Option<String>,
    pub import_ids: Vec<String>,
    pub description: Option<String>,
    pub boundaries: Option<Boundaries>,
    pub constraints: Vec<Constraint>,
    #[serde(skip)]
    pub spec_path: Option<PathBuf>,
}
```

### `Boundaries` additions

```rust
pub struct Boundaries {
    pub path: Option<String>,
    pub public_api: Vec<String>,

    // importer-side
    pub allow_imports_from: Vec<String>,
    pub never_imports: Vec<String>,
    pub allow_type_imports_from: Vec<String>,

    // provider-side
    pub visibility: Option<Visibility>,   // default Public
    pub allow_imported_by: Vec<String>,
    pub deny_imported_by: Vec<String>,
    pub friend_modules: Vec<String>,

    // canonical import policy
    pub enforce_canonical_imports: bool,

    // dependencies
    pub allowed_dependencies: Vec<String>,
    pub forbidden_dependencies: Vec<String>,

    pub enforce_in_tests: bool,
}

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Internal,
    Private,
}
```

---

## 3. Rule Precedence Contract (must be implemented as-is)

For cross-module edge A -> B:

1. `A.never_imports` deny
2. `B.deny_imported_by` deny
3. `B.visibility` gate (`private`/`internal`/`public`, with `friend_modules` exception)
4. `B.allow_imported_by` allowlist (if non-empty)
5. `A.allow_imports_from` allowlist (if non-empty)
6. type-only exemption via `A.allow_type_imports_from`

Deny wins over allow at every step.

---

## 4. Canonical Import IDs

Resolver/spec index must expose canonical IDs per module:

- preferred: `import_id`
- aliases: `import_ids`
- optional package hint: `package`

### Enforcement behavior

If target module has `enforce_canonical_imports: true`, then:

- cross-module relative imports are violations,
- import must use one of target canonical IDs,
- same-module relative imports remain valid.

Violation rule id: `boundaries.canonical_imports`.

---

## 5. Parser / Dependency Extraction Widening

### Required extracted edges

- static import/export from string literals
- `require("literal")`
- `import("literal")` (string literal only)

### Warning behavior

- `import(expr)` with non-literal argument -> `resolver.unresolved_dynamic_import` warning.

### `jest.mock` behavior

Config key in `specgate.config.yml`:

```yaml
jest_mock_mode: warn # warn | enforce
```

- `warn` (default): emit telemetry warning only.
- `enforce`: treat literal `jest.mock("x")` as dependency edge for rule evaluation.

---

## 6. Resolver Parity Command

Add doctor subcommand:

```bash
specgate doctor compare --from src/a.ts --import @/x
```

It must print:

- Specgate (`oxc_resolver`) result + step trace
- `tsc --traceResolution` extracted result
- parity verdict: `MATCH` / `DIFF`
- actionable mismatch hint (tsconfig paths, conditions, package exports, symlink)

This command is diagnostic; it does not affect check exit codes.

---

## 7. Baseline Mechanism

### CLI

```bash
specgate baseline --write .specgate-baseline.json
```

### Baseline file format

```json
{
  "version": "1",
  "generated_from": {
    "tool_version": "0.1.0",
    "git_sha": "abc123",
    "config_hash": "...",
    "spec_hash": "..."
  },
  "fingerprints": [
    "sha256:..."
  ]
}
```

### Fingerprint generation

Hash normalized tuple:

`module|rule|severity|file|line|import_source|resolved_target`

(normalized path separators, repo-root-relative paths).

### CI behavior

- if violation fingerprint exists in baseline: report-only
- if not in baseline: enforce by severity
- summary must include:
  - `baseline_hits`
  - `new_violations`
  - optional `stale_baseline_entries`

---

## 8. Determinism Contract (fixed)

### Output mode enum

```rust
pub enum OutputMode {
    Deterministic, // default
    Metrics,
}
```

### Deterministic mode requirements (default)

- Byte-identical for same inputs
- Include only stable fields (e.g., `tool_version`, `git_sha`, `config_hash`, `spec_hash`, sorted `violations`)
- Exclude `timestamp`, `duration_ms`, host/process identifiers

### Metrics mode requirements

- Adds runtime telemetry (`timestamp`, `duration_ms`, optional perf counters)
- Must not change violation ordering/content

CLI:

```bash
specgate check --output-mode deterministic   # default
specgate check --output-mode metrics
```

---

## 9. Verdict Model Additions

Add governance and baseline visibility fields:

```rust
pub struct Verdict {
    pub specgate: String,
    pub output_mode: String,
    pub tool_version: String,
    pub git_sha: String,
    pub config_hash: String,
    pub spec_hash: String,

    pub verdict: VerdictStatus,
    pub violations: Vec<Violation>,

    pub baseline_hits: usize,
    pub new_violations: usize,

    pub spec_files_changed: Vec<PathBuf>,
    pub rule_deltas: Vec<RuleDelta>,
    pub policy_change_detected: bool,

    // metrics mode only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}
```

`RuleDelta` minimally contains: `spec`, `rule`, `change_type`, `before`, `after`.

---

## 10. Check Pipeline (updated)

1. Load config/specs and validate schema/version `2.2`
2. Build resolver (`oxc_resolver`) + module index (includes canonical IDs)
3. Parse source files and build dependency graph (file edges)
4. Evaluate rules with updated precedence model
5. Apply baseline classification (hit/new)
6. If diff mode, compute:
   - affected modules
   - `spec_files_changed`
   - `rule_deltas`
7. Build verdict in requested output mode
8. Exit codes:
   - `0` pass
   - `1` fail (new error violations)
   - `2` config/parse/runtime setup failure

---

## 11. CLI Surface (v1.1)

- `specgate check [--diff <ref>] [--output-mode deterministic|metrics]`
- `specgate validate`
- `specgate init`
- `specgate doctor`
- `specgate doctor compare --from <file> --import <specifier>`
- `specgate baseline --write <path>`

---

## 12. Testing Requirements Additions

Add integration fixtures for:

- provider visibility precedence conflicts
- canonical import enforcement (cross-module relative ban)
- baseline hit vs new violation behavior
- deterministic output byte-identical across repeated runs
- metrics mode field presence without semantic drift
- `require("literal")` and literal `import()` extraction
- non-literal dynamic import warning
- `jest.mock` warn vs enforce modes
- `doctor compare` parity match/mismatch snapshots

---

## 13. MVP Scope Guardrails

- Keep Phase 1 rule engine file-edge based.
- Do not build generalized deep symbol-origin propagation now.
- Introduce Phase 1.5 task for deep re-export symbol provenance only if needed by concrete rule pressure.

---

## 14. Rust Direction (explicitly unchanged)

- Continue Rust implementation with `oxc_parser`/`oxc_resolver`.
- Do not migrate MVP core to TypeScript.
- Prioritize resolver correctness/parity diagnostics and deterministic contract reliability.

---

## 15. Remaining Work (Prioritized)

1. **MVP merge gate definition + CI wiring (P0)**
   - Add a single documented "MVP-ready" gate that combines:
     - contract fixtures,
     - golden corpus,
     - Tier A deterministic gate,
     - baseline/new-violation behavior.
   - Ensure CI exposes clear fail reasons (`policy` vs `runtime` vs `contract drift`).

2. **Golden corpus expansion beyond scaffold (P0/P1)**
   - Current top-5 corpus scaffold is merged and passing.
   - Expand to broader failure classes and map each case to one explicit rule contract.
   - Keep intro/fix pairs stable and reproducible.

3. **Doctor UX parity and operator trust tooling (P1)**
   - Finalize `doctor compare` ergonomics and mismatch diagnostics for monorepos/project refs.
   - Tighten guidance quality so users can resolve parity issues without source-diving.

4. **Governance hardening completeness (P1)**
   - Ensure spec-change governance fields (`spec_files_changed`, `rule_deltas`, `policy_change_detected`) are consistently surfaced in diff-aware workflows.
   - Document suppression lifecycle and stale baseline hygiene policy.

5. **Docs consolidation and adoption path (P1)**
   - Unify contract docs (`WAVE0_CONTRACT.md`), fixture design docs, and this implementation plan into one operator-facing onboarding path.

---

## 16. Learned During Build / Plan Additions

1. **Precision requires explicit near-miss fixtures, not just fail/pass fixtures.**
   - Added near-miss expansion in Tier A hardening to protect against false positives.
   - Plan addition: every Tier A case should include at least one near-miss variant.

2. **Null semantics must be contract-tested for deterministic JSON consumers.**
   - A06 now requires explicit `to_module: null` (key present + null value), not omitted field.
   - Plan addition: represent nullable contract fields explicitly and lock with tests.

3. **Determinism gates need harness-level strictness, not just ad-hoc spot checks.**
   - Tier A strict deterministic gate plus fixture-level assertions proved valuable.
   - Plan addition: keep deterministic gate as required pre-merge check for contract-sensitive changes.

4. **Semantic docs + tests must co-evolve in the same wave.**
   - Wave 0 lock succeeded because CLI semantics, docs, and tests were merged together.
   - Plan addition: treat semantic changes as incomplete until doc + contract fixture + integration coverage land together.

5. **Git-aware blast-radius and baseline semantics are high-leverage trust features.**
   - They move Specgate from static checker to practical CI gate by reducing noise while preserving safety.
   - Plan addition: prioritize operator clarity around these two workflows in user docs and examples.

---

## 17. Open Implementation Questions

1. Should `friend_modules` bypass `allow_imported_by` or only visibility gates? (recommended: bypass visibility only, still subject to explicit deny)
2. Baseline lifecycle policy: auto-prune stale fingerprints vs manual review gate?
3. `doctor compare` implementation detail: parse `tsc --traceResolution` text robustly in monorepos with project references.

These do not block v1.1 implementation start.
