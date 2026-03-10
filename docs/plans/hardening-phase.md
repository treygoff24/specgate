# Hardening Phase — Post-Phase 5 Roadmap

**Date:** 2026-03-07
**Author:** Lumen (Opus 4.6), synthesized from GPT 5.4 Pro external review + internal assessment
**Sequence:** After Phase 5 (Envelope AST Check) is complete
**Source review:** GPT 5.4 Pro architectural review (no repo access, partial context)

---

## Context

An external review of Specgate by GPT 5.4 Pro produced 17 findings. After cross-referencing each finding against the actual codebase, approximately 40% were already addressed, 30% were partially right, and 30% were genuinely new insights. This document captures the actionable items worth building, organized by priority.

Items marked **[ALREADY EXISTS]** note where the reviewer missed existing functionality so we don't duplicate work. Items marked **[NEW]** are genuinely new capabilities.

---

## Priority 1: CLI Refactor

**Source:** Finding #5 (high confidence, confirmed)
**Status:** [NEW] — Real tech debt

`src/cli/mod.rs` is 2,800+ lines handling argument parsing, command dispatch, reporting, baseline handling, exit semantics, and policy-adjacent logic. This slows down every future change.

### What to build

Split into clearly separated modules:

```
src/cli/
├── mod.rs              # Command dispatch only
├── args.rs             # Argument parsing, flag definitions
├── check.rs            # `specgate check` command handler
├── init.rs             # `specgate init` command handler
├── doctor.rs           # `specgate doctor` command handler
├── baseline.rs         # Baseline management (update, diff, stale policy)
├── diff.rs             # CI/diff integration (--since, blast radius)
└── render/
    ├── mod.rs           # Renderer trait
    ├── human.rs         # Human-readable terminal output
    ├── json.rs          # JSON verdict output
    ├── ndjson.rs        # NDJSON streaming output
    └── sarif.rs         # SARIF output (see Priority 4)
```

**Key invariant:** All output formats render from the same internal `Verdict` model. No format-specific logic in command handlers.

**Why first:** Every subsequent hardening item touches the CLI. Refactoring it first prevents compounding tech debt.

---

## Priority 2: Policy Governance

**Source:** Finding #9 (high confidence, genuinely new)
**Status:** [NEW] — Highest-value new capability

Once agents learn that policy blocks them, the natural failure mode is "widen the rule" instead of "fix the code." Specgate needs to guard policy against convenience-driven weakening, not just guard code against policy.

### What to build

**Spec change detection:** A new analysis mode that compares spec files between two commits and classifies each change:
- **Widening:** New `allow_imports_from` entries, removed `never_imports` rules, new `public_api` exports, relaxed `envelope: required` → `optional`
- **Narrowing:** Removed permissions, added restrictions, new `never_imports` rules
- **Organizational:** Renames, comment changes, reordering without semantic change

**Integration points:**
- `specgate policy-diff --base origin/main` — CLI command showing what changed and how
- Machine-readable output for CI: `{"change_type": "widening", "module": "api/handlers", "field": "allow_imports_from", "added": ["database/internal"]}`
- CODEOWNERS integration guidance: recommend requiring senior/architect review for widening changes
- Future: `specgate check --deny-widenings` flag that fails CI if the PR widens policy without an explicit override

**Why this matters:** This is the one capability that no other tool in the ecosystem provides. ESLint, dependency-cruiser, etc. all enforce policy but none detect when policy itself is being eroded.

---

## Priority 3: Adversarial Fixture Zoo

**Source:** Finding #16 (high confidence, genuinely high leverage)
**Status:** [NEW] — Highest-leverage testing improvement

Current fixtures test correctness. They don't test adversarial agent behavior. Build a fixture suite around how agents actually fail.

### What to build

```
tests/fixtures/adversarial/
├── cross-layer-shortcut/        # Handler imports directly from DB layer (skipping service)
├── deep-third-party-import/     # import { internal } from 'express/lib/router/index'
├── test-helper-leak/            # Test utility imported in production code
├── dynamic-import-evasion/      # Dynamic import used to bypass static boundary check
├── policy-widening-pr/          # PR that widens spec alongside violating code
├── ownership-overlap/           # Two specs claim the same file via overlapping globs
├── orphan-module/               # Spec file with no matching source files
├── barrel-re-export-chain/      # A→B→C→D re-export chain that hides the real dependency
├── type-import-downgrade/       # Type-only import changed to value import in same PR
├── circular-via-re-export/      # Circular dependency created through barrel file re-exports
├── aliased-deep-import/         # Path alias resolves to a deep internal of another module
└── conditional-require/         # require() inside if block that only runs in certain environments
```

Each fixture should have:
- The source files representing the agent mistake
- A `.spec.yml` that should catch it
- Expected violations (or documentation of why specgate can't catch it yet)
- A brief description of the real-world scenario

**Test file:** `tests/adversarial_fixtures.rs`

**Why third:** Depends on nothing, but benefits from CLI refactor being done first (cleaner test output).

---

## Priority 4: SARIF Output

**Source:** Finding #13 (high confidence, correct for adoption)
**Status:** [NEW]

GitHub Code Scanning ingests SARIF 2.1.0. Native SARIF output gives specgate a first-class home in GitHub's security and PR review surfaces without any middleware.

### What to build

- SARIF 2.1.0 renderer in `src/cli/render/sarif.rs` (post CLI refactor)
- `specgate check --format sarif` flag
- Stable rule IDs as SARIF `reportingDescriptor.id`
- Violation fingerprints as SARIF `result.fingerprints` for baseline stability
- File locations as SARIF `physicalLocation` with line/column when available
- GitHub Actions workflow example: run specgate, upload SARIF artifact

**Dependencies:** CLI refactor (Priority 1) — the renderer abstraction needs to exist first.

---

## Priority 5: Ownership Registry Improvements

**Source:** Finding #2 (partially right — ownership IS explicit via `path` globs, but gaps exist)
**Status:** [PARTIAL] — Incremental improvements

### What exists
- Module ownership via `path` field in spec files (glob-based)
- `doctor` command for general diagnostics

### What to build

Add a **module registry validation pass** that runs before graph evaluation and catches:

| Check | Status | Priority |
|-------|--------|----------|
| File-to-module ownership resolution | ✅ Exists | — |
| Overlapping ownership (two specs claim same file) | ✅ Exists (`doctor ownership`) | High |
| Unclaimed file detection (source file owned by no module) | ✅ Exists (`doctor ownership`) | High |
| Duplicate module ID detection | ✅ Exists (`doctor ownership`) | Medium |
| Orphaned spec detection (spec with no matching files) | ✅ Exists (`doctor ownership`) | Medium |
| Contradictory glob detection | ❌ Missing | Low |

**New command:** `specgate doctor ownership` — explains ownership resolution for every tracked source file, independent of policy evaluation.

**Config option:** `strict_ownership: true` — makes ownership findings fail the command in CI instead of remaining informational-only.

---

## Priority 6: Unknown Edge Classification

**Source:** Finding #4 (partially right — dynamic imports ARE tracked, but verdict output doesn't expose taxonomy)
**Status:** [PARTIAL]

### What exists
- `dynamic_imports` and `dynamic_warnings` in `FileAnalysis`
- `require_calls` tracking
- Re-export edge detection

### What to build

Explicit edge classification in verdict output:

```json
{
  "edge_classification": {
    "resolved": 342,
    "unresolved_literal": 3,
    "unresolved_dynamic": 7,
    "external": 28,
    "ignored": 2,
    "baselined": 1
  },
  "unresolved_edges": [
    {
      "from": "src/api/handler.ts",
      "specifier": "./missing-module",
      "kind": "unresolved_literal",
      "line": 42
    }
  ]
}
```

**Config option:** `unresolved_edge_policy: "warn" | "error" | "ignore"` — let repos choose whether unresolved edges are warnings or hard failures.

---

## Priority 7: Baseline v2 Enhancements

**Source:** Finding #7 (partially right — baseline exists, missing some fields)
**Status:** [PARTIAL]

### What exists
- Baseline fingerprinting with `--baseline-diff`
- Stale baseline policies (`stale_baseline` config)
- Escape hatch system with `require_expiry` and `max_new_per_diff`

### What to build

Extend baseline entries with:
- `owner` field (who suppressed this violation)
- `reason` field (why it's suppressed)
- Individual entry expiry dates (currently only global `require_expiry`)
- Expired suppression → automatic failure in CI

These are incremental additions to the existing baseline model, not a rewrite.

---

## Priority 8: Provider-Side Visibility Model

**Source:** Finding #10 (partially right — `public_api` and `never_imports` exist)
**Status:** [PARTIAL]

### What exists
- `public_api` declarations (which files a module exposes)
- `never_imports` rules (which modules are forbidden)

### What to build

Richer visibility model on the provider side:

```yaml
# In module spec
visibility: internal    # public | internal | private

# Allow specific consumers (friendship)
allow_consumers:
  - core/auth
  - shared/utils
```

- `public` — any module can import (current default)
- `internal` — only modules in the same parent namespace can import
- `private` — only explicitly listed consumers can import

This gives module authors control over their boundaries even when consumer specs are incomplete.

---

## Priority 9: Import Hygiene Rules

**Source:** Finding #11 (new, forward-looking)
**Status:** [NEW]

### What to build

First-class rules for common agent mistakes that aren't boundary violations per se:

- **Deep third-party imports:** `import { x } from 'express/lib/internal/thing'` — flag imports that bypass a package's public entrypoint
- **Canonical import enforcement:** Require imports to use the shortest/canonical path, not deep internal paths
- **Test-production boundary:** Flag test utilities imported in production code

These could be configurable rules in the spec:

```yaml
import_hygiene:
  deny_deep_imports:
    - "express/**"
    - "react/**"
  test_boundary:
    test_patterns: ["**/*.test.ts", "**/__tests__/**"]
    deny_production_imports: true
```

---

## Items NOT worth building (reviewer was wrong)

| Finding | Why it's wrong |
|---------|---------------|
| #1 — Determinism not airtight | Already deterministic. `stable_unique()`, sorted output, telemetry separated, golden corpus gate tests. |
| #3 — Replace resolver with TS compiler API | We use `oxc_resolver` (ecosystem standard, used by Biome/Rspack). Adding a Node.js dependency would destroy performance. `doctor compare` validates parity. |
| #14 — Release attestations | Premature for current stage. Worth doing before 1.0. |
| #15 — CodeQL on the repo | Nice-to-have but low ROI given the codebase size and Rust's type safety. |

---

## Items to note but not build yet

| Finding | Disposition |
|---------|------------|
| #6 — Don't get too clever | General advice, not actionable. We're already focused on correctness over features. |
| #12 — Doc drift | Just addressed via repo reorganization (2026-03-07). Could add schema-generated docs later. |
| #17 — Starter rule packs | Good for adoption but premature before dogfooding on OpenClaw. |
| #8 — PR-native diff mode | `--since` blast-radius exists. The transitive-affected report and new/resolved/unchanged classification are nice-to-haves, partially covered by Priority 6. |
