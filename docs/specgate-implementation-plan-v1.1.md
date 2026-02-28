# Specgate Implementation Plan

**Version:** 1.1 — February 26, 2026  
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

## Build Execution Status Refresh (2026-02-26, post-swarm integration)

This section updates the plan against final merged `master` history and validation results.

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
- [x] **All swarm lanes merged and pushed to `master`**
  - Final integrated head: `341ebc39728390ee1e163da4ab14efbb8ba8f219`.
  - Landed lane branches / commits:
    - `feature/mvp-gate-ci` (includes `2c3187d`)
    - `feature/golden-corpus-expansion` (`edcff68`, `6528deb`)
    - `feature/governance-hardening` (`7379cc8`)
    - `feature/doctor-ux-parity` (`89459b1`)
    - `feature/docs-consolidation` (`126bc38`, `502ad8a`)

### Current verification snapshot

- Final merged `master` validation: **PASS**
  - `cargo fmt --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test` (129 unit + 12 contract fixtures + 14 golden corpus + 28 integration + 2 mvp_gate_baseline + 1 Tier A + 10 wave2c CLI)
  - `./scripts/ci/mvp_gate.sh`
- Branch state: `master` clean and aligned with `origin/master` at `341ebc39728390ee1e163da4ab14efbb8ba8f219`.

### MVP completion estimate

- **~95% complete as of 2026-02-26, ship-ready for shipping/dogfooding with explicit hardening tasks below.**
- Core MVP scope is implemented and validated on merged `master`; remaining work is release/operations hardening, policy finalization, and adoption follow-through.

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
specgate baseline --output .specgate-baseline.json
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
  "entries": [
    {
      "fingerprint": "sha256:...",
      "positional_fingerprint": "sha256:...",
      "rule": "boundary.allow_imports_from",
      "severity": "error",
      "message": "...",
      "from_file": "src/api/handlers/user.ts",
      "to_file": "src/infra/db/index.ts",
      "from_module": "core/api",
      "to_module": "infra/db",
      "line": 15,
      "column": 8
    }
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
  - `summary.baseline_violations`
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
- Include stable fields only (e.g., `tool_version`, `git_sha`, `config_hash`, `spec_hash`, `summary`, sorted `violations`)
- Exclude runtime `metrics` section and duration fields

### Metrics mode requirements

- Adds runtime telemetry in `metrics` (`timings_ms`, `total_ms`) and optional perf counters
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
    pub schema_version: String,
    pub tool_version: String,
    pub git_sha: String,
    pub config_hash: String,
    pub spec_hash: String,
    pub output_mode: String,
    pub spec_files_changed: Vec<String>,
    pub rule_deltas: Vec<String>,
    pub policy_change_detected: bool,
    pub status: VerdictStatus,
    pub summary: VerdictSummary,
    pub violations: Vec<VerdictViolation>,

    // metrics mode only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<VerdictMetrics>,
}
```

```rust
pub struct VerdictSummary {
    pub total_violations: usize,
    pub new_violations: usize,
    pub baseline_violations: usize,
    pub suppressed_violations: usize,
    pub error_violations: usize,
    pub warning_violations: usize,
    pub new_error_violations: usize,
    pub new_warning_violations: usize,
    pub stale_baseline_entries: usize,
}
```

`rule_deltas` are represented in JSON as governance labels.

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

- `specgate check [--since <ref>] [--output-mode deterministic|metrics]`
- `specgate validate`
- `specgate init`
- `specgate doctor`
- `specgate doctor compare --from <file> --import <specifier>`
- `specgate baseline --output <path>`

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

1. **Release-candidate hardening and reproducibility evidence (P0)**
   - Cut an MVP release candidate from the current verified `master` tip (or latest stable release branch point) with attached gate artifacts.
   - Run clean-room verification (fresh clone + documented command path) to confirm onboarding/reproducibility.

2. **Baseline lifecycle policy finalization (P0/P1)** ✅ DECIDED 2026-02-27
   - **Stale entry policy:** Default `warn` — CI passes but reports stale baseline entries. Config flag `stale_baseline: fail` available for teams wanting strict hygiene. Auto-prune rejected (loses audit trail; can't distinguish "fixed" from "silently disappeared").
   - **Refresh cadence:** Operator-defined. Specgate surfaces stale counts; teams decide review rhythm (per-release recommended, weekly for strict shops).
   - **CLI:** `specgate baseline --refresh` to prune stale entries after review. `specgate check` output includes `stale_baseline_entries` count in summary.

3. **Doctor parity depth for monorepos/project references (P1)**
   - Add focused fixtures covering project references and complex path alias overlaps.
   - Keep mismatch hints high-signal so parity diffs can be resolved without source spelunking.

4. **Governance UX polish in diff workflows (P1)**
   - Improve readability of `spec_files_changed` / `rule_deltas` in human output for review-time decisions.
   - Ensure policy-change context is obvious in both local and CI surfaces.

5. **Operator adoption closeout (P1)**
   - Validate end-to-end onboarding docs against a new-user walkthrough.
   - Publish concise "MVP gate + baseline hygiene" guidance as the default operational path.

### Tomorrow pickup checklist

- [x] Tag `v0.1.0-rc3` from the current verified head after quick smoke verification.
- [x] Capture and archive gate artifacts for that SHA (`fmt`, `clippy`, `test`, `scripts/ci/mvp_gate.sh`) at `docs/release-artifacts/v0.1.0-rc3-gate-evidence.md` (historical `v0.1.0-rc2` evidence remains in `docs/release-artifacts/v0.1.0-rc2-gate-evidence.md`, with `v0.1.0-rc1` evidence in `docs/release-artifacts/v0.1.0-rc1-gate-evidence.md`).
- [x] Decide stale baseline policy and document enforce/warn behavior in one canonical location. ✅ DECIDED 2026-02-27 (warn default, opt-in fail, no auto-prune).
- [x] Add one monorepo/project-reference `doctor compare` fixture that currently lacks coverage.
- [x] Run docs onboarding from a clean clone and fix any command or path ambiguity found.
- [x] Draft release notes summarizing landed lanes, rule-impacting changes, and operator action items.

## 19. Release closeout (Current snapshot: 2026-02-26)

- ✅ Completed MIT license file and placeholder replacement in `README.md`.
- ✅ Added closeout docs: `CHANGELOG.md`, `RELEASING.md`, `RELEASE_NOTES.md`.
- ✅ Added dogfood docs: `BASELINE_POLICY.md`, `DOGFOOD_ROLLOUT_CHECKLIST.md`,
  `DOGFOOD_SUCCESS_METRICS.md`, `DOGFOOD_RELEASE_CHANNEL.md`.
- ✅ Added copy-paste consumer workflow example:
  `docs/examples/specgate-consumer-github-actions.yml`.
- ✅ Clarified explicit deferred rule classes (`C02`, `C06`, `C07`) in documentation.
- ⏳ Remaining: monitor deferred rule families and close any operational ambiguity
  before moving from dogfood to broader stable adoption.

---

## 17. Learned During Build / Plan Additions

1. **Merge-train delivery worked because each lane shipped one trust boundary end-to-end.**
   - CI gate, corpus expansion, governance, doctor UX, and docs each landed as independently testable slices.
   - Plan addition: keep future waves lane-scoped with explicit merge commits + SHA traceability in this plan.

2. **Deterministic confidence requires both broad corpus and strict gate mechanics.**
   - Tier A strict determinism + expanded golden corpus caught regressions that smaller fixture sets would miss.
   - Plan addition: every new rule path should add at least one intro/fix pair and one near-miss precision case.

3. **Baseline usefulness depends on stale-entry hygiene, not just hit/new classification.**
   - Stale tracking landed, but policy decisions (enforcement threshold/cadence) are still operator-critical.
   - Plan addition: treat baseline hygiene as an operational control with explicit owner and review cadence.

4. **Parity diagnostics must optimize for remediation speed, not raw resolver detail.**
   - `doctor compare` became materially more useful once output centered on focused mismatch verdicts and hints.
   - Plan addition: new parity work should be judged by "time-to-fix" in real monorepo cases.

5. **Docs are part of the enforcement surface.**
   - Consolidated docs and gate runbook reduced ambiguity in what "MVP-ready" means.
   - Plan addition: no gate or policy change should merge without same-wave operator docs updates.

---

## 18. Open Implementation Questions

1. ~~Should `friend_modules` bypass `allow_imported_by` or only visibility gates?~~ — ✅ DECIDED 2026-02-27. **Friends bypass visibility gates only.** Explicit denies (`deny_imported_by`) always win. `allow_imported_by` allowlists are NOT bypassed by friends — if you set an allowlist, friends still need to be on it. Principle: deny always wins, friends are a visibility exemption, not a superuser pass.
2. ~~Baseline lifecycle policy~~ — ✅ DECIDED 2026-02-27. See §15 item 2.
3. `doctor compare` implementation detail: parse `tsc --traceResolution` text robustly in monorepos with project references. — **OPEN. Implementation detail, not blocking.**

Only question 1 requires a design decision before broader rollout.

---

## 18. Post-MVP: Architectural Pattern Scanning

**Status:** Deferred until after MVP.

Beyond import-graph boundary enforcement, Specgate could scan for broader architectural patterns via regex — detecting usages of deprecated APIs, convention violations, naming rule enforcement, or custom pattern rules defined in spec files. This would extend Specgate from "did you respect the import graph?" to "did you respect the project's conventions?"

**Candidate dependency:** [`ripgrep-api`](https://crates.io/crates/ripgrep-api) by Alex Younger (@AlextheYounga) — a clean Rust API wrapper around ripgrep's core crates. Builder pattern, structured `Match` results with file/line/text, glob filtering, threading, mmap support, streaming callbacks. Well-designed for programmatic use. Not needed for MVP (import resolution requires AST, not regex), but a natural fit for pattern scanning.

**GitHub:** https://github.com/AlextheYounga/ripgrep-api
