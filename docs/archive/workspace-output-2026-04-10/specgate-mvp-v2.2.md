# Specgate: MVP Specification v2.2

### Machine-Checkable Architectural Intent for Agent-Generated Code

**Version:** 2.2 — February 25, 2026  
**Status:** Ready for build (Rust MVP)

---

## Changelog (v2.2)

- Fixed determinism contract: default output is byte-identical and excludes run-time metrics; optional metrics mode adds timestamp/duration.
- Added positioning vs Nx / dependency-cruiser / JS Boundaries / good-fences.
- Added provider-side visibility controls: `visibility`, `allow_imported_by`, `deny_imported_by`, `friend_modules`.
- Added canonical import identifiers: `import_id`, `import_ids`, `package`; optional canonical cross-module import enforcement.
- Added baseline workflow: `specgate baseline` + fingerprinted known violations.
- Added spec-collusion governance: spec diffs/rule deltas are first-class verification output.
- Expanded dependency extraction to include `require("literal")` and `import("literal")`; non-literal dynamic imports become unresolved warnings; clarified `jest.mock` behavior.
- Added `specgate doctor compare` parity command vs `tsc --traceResolution` for one import.
- Trimmed MVP risk: keep MVP file-edge based; defer deep symbol-origin tracing through long re-export chains to Phase 1.5 unless required.
- Kept Rust implementation direction, framed around `oxc_resolver` parity/correctness.

---

## 1. Problem Statement

AI coding agents generate code faster than humans can verify architectural correctness. Existing checks (types, lint, tests) catch many local defects but routinely miss structural intent violations:

- cross-layer imports,
- bypassed public entrypoints,
- forbidden dependency use,
- policy drift where test and implementation are co-authored by the same agent.

Specgate closes this by making architectural intent machine-checkable and deterministic.

---

## 2. Core Insight

1. Intent already exists (in tickets/specs/review comments) but is not encoded for deterministic checking.
2. Verification must be independent of the generator.
3. Most high-frequency agent regressions are structural and statically checkable.
4. Verification output must be stable enough for automation loops (CI, bots, repair agents).

---

## 3. Positioning (Why Specgate, not existing tools)

- **Nx**: great task graph/orchestration; not a deterministic policy-verdict substrate with machine-readable repair-loop artifacts.
- **dependency-cruiser**: strong graph constraints, but not designed as an agent-first governance contract with spec-collusion visibility and baseline governance semantics.
- **JS Boundaries / good-fences**: useful boundary linting, but generally linter-centric; Specgate is CI-verdict-centric with deterministic output modes, explicit policy governance fields, and first-class automation contracts.

**Specgate’s differentiator:** an **agent-first deterministic verification substrate** with governance primitives and machine-readable output designed for automatic remediation loops.

---

## 4. MVP Scope (Phase 1)

Structural policy engine for TypeScript/JavaScript projects, implemented in Rust:

- Module boundary enforcement
- Public entrypoint enforcement
- Third-party dependency controls
- Layer rules
- Circular dependency detection
- Type-only import exceptions
- Diff-aware affected-module evaluation

### Scope trim (risk control)

- **MVP stays file-edge based** (imports/re-exports/dependency edges).
- **Deep symbol-origin tracing across long re-export chains is deferred to Phase 1.5**, unless a specific rule explicitly requires it.

---

## 5. Core Engineering Priority: Resolution Correctness

`oxc_resolver` parity with TypeScript/Node resolution is the highest-priority engineering area. Incorrect resolution invalidates every downstream rule.

Must support:

- `tsconfig` `paths`/`baseUrl`
- extension and `index.*` fallback
- workspace package/symlink resolution
- barrel/re-export chains at file-edge granularity

Diagnostic commands:

- `specgate doctor` (general)
- `specgate doctor compare --from <file> --import <specifier>` compares Specgate resolution vs `tsc --traceResolution` for one import.

---

## 6. Spec Language (v2.2)

```yaml
# specgate.schema: v2.2
version: "2.2"

module: string
package: string?              # optional package/workspace identifier
import_id: string?            # canonical import id (single)
import_ids:                   # canonical import ids (multiple aliases)
  - string

description: string?

boundaries:
  path: string
  public_api:
    - string

  # Importer-side controls
  allow_imports_from:
    - string
  never_imports:
    - string
  allow_type_imports_from:
    - string

  # Provider-side controls
  visibility: public|internal|private     # default: public
  allow_imported_by:
    - string
  deny_imported_by:
    - string
  friend_modules:
    - string

  # Canonical import enforcement
  enforce_canonical_imports: bool          # default: false

  # Third-party dependencies
  allowed_dependencies:
    - string
  forbidden_dependencies:
    - string

constraints:
  - rule: string
    params: object
    severity: error|warning
    message: string?
```

### Rule precedence (importer + provider)

For a cross-module import A -> B, evaluate in this order:

1. Importer `never_imports` (hard deny)
2. Provider `deny_imported_by` (hard deny)
3. Provider `visibility`:
   - `private`: only same module
   - `internal`: same package/workspace (plus `friend_modules`)
   - `public`: no extra restriction
4. Provider `allow_imported_by` (if non-empty, default-deny allowlist)
5. Importer `allow_imports_from` (if non-empty, default-deny allowlist)
6. Type-only carve-out via importer `allow_type_imports_from`

`deny` always overrides `allow`.

### Canonical import IDs

- Modules can publish canonical IDs (`import_id` / `import_ids`, optionally `package`).
- If `enforce_canonical_imports: true`, cross-module imports must use canonical IDs, not relative cross-module paths.
- Same-module relative imports remain allowed.

Example violation: `../../orders/internal/x` from another module when canonical id is `@app/orders`.

---

## 7. Dependency Edge Extraction Rules

MVP extracts file-edge dependencies from:

- `import ... from "literal"`
- `export ... from "literal"`
- `require("literal")`
- `import("literal")` where argument is a string literal

Warnings:

- `import(expr)` with non-literal argument => unresolved dynamic import warning (no hard failure by default)
- `jest.mock("literal")`:
  - default: warning-only telemetry edge (does not enforce boundary/dependency rules)
  - optional config: `jest_mock_mode: enforce | warn` to include in rule evaluation

---

## 8. Baseline Mechanism

### Command

```bash
specgate baseline --write .specgate-baseline.json
```

Generates known-violation fingerprints used to prevent noisy legacy debt from blocking rollout.

### Fingerprint definition

A fingerprint is a stable hash over normalized fields:

`module | rule | severity | normalized_file | line | normalized_import_source | normalized_resolved_target`

Normalization removes machine-local path prefixes and uses `/` separators.

### CI behavior

- Violation fingerprint in baseline => **report-only**
- Violation fingerprint not in baseline => enforced by severity (new errors fail CI)
- Optional strict mode can fail on baseline drift or stale entries

---

## 9. Output Contract (Deterministic + Metrics Modes)

### Default mode: deterministic (byte-identical)

Default JSON excludes run-variant fields like timestamp/duration.

```json
{
  "specgate": "2.2",
  "output_mode": "deterministic",
  "tool_version": "0.1.0",
  "git_sha": "abc123",
  "config_hash": "...",
  "spec_hash": "...",
  "verdict": "FAIL",
  "violations": ["...sorted..."],
  "summary": "3 errors, 1 warning"
}
```

### Optional mode: metrics

```bash
specgate check --output-mode metrics
```

Adds non-deterministic telemetry fields:

- `timestamp`
- `duration_ms`
- optional perf counters

Both modes keep identical violation semantics; only payload shape differs.

---

## 10. Spec-Collusion Governance

Spec updates are first-class policy events, not hidden context.

Guidance:

- Treat spec changes as equivalent to code-policy changes.
- In CI, optionally require approval/label for spec-changing PRs (team policy).

Required output fields in PR-scope mode:

- `spec_files_changed: string[]`
- `rule_deltas: [{spec, rule, change_type, before, after}]`
- `policy_change_detected: boolean`

This prevents silent “change-the-spec-to-pass” collusion patterns.

---

## 11. Escape Hatch Governance

`@specgate-ignore` remains allowed with controls:

- reason required
- optional expiry (`until:YYYY-MM-DD`)
- report active/new/expired ignores
- configurable max new ignores per diff

---

## 12. CLI (MVP)

- `specgate check`
- `specgate check --diff <ref>`
- `specgate validate`
- `specgate init`
- `specgate doctor`
- `specgate doctor compare --from <file> --import <specifier>`
- `specgate baseline --write .specgate-baseline.json`

---

## 13. Rust Implementation Direction (unchanged)

Implementation remains Rust-first with `oxc_parser` + `oxc_resolver`.

- No switch to TypeScript implementation.
- Verification core remains deterministic and non-LLM.
- Resolver parity with TypeScript remains explicit via doctor-compare diagnostics.

---

## 14. Performance Targets

- 50 files / 5 specs: <2s
- 500 files / 30 specs: <5s
- 5000+ files / 100+ specs: <30s
- Diff-aware small changes: <1s (with graph cache)

---

## 15. Out of Scope (MVP)

- Deep symbol provenance through arbitrary re-export chains (Phase 1.5)
- Runtime invariant enforcement (Phase 2)
- Behavioral verification (Phase 3)
- Non-TS/JS language frontends (post-MVP)

This keeps MVP focused, deterministic, and shippable.
