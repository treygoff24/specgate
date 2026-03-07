# TS/JS Ecosystem Support — Final Build-Ready Implementation Plan

Date: 2026-03-03  
Repo: `~/Development/specgate`  
Inputs synthesized:
- Original plan: `docs/tsjs-ecosystem-plan.md` (Vulcan xhigh)
- Architecture review: `docs/tsjs-ecosystem-plan-opus-review.md` (Opus)
- Resolver feasibility review: `docs/tsjs-ecosystem-plan-resolver-review.md` (Athena/Gemini)
- Verified source files:
  - `src/resolver/mod.rs`
  - `src/resolver/classify.rs`
  - `src/parser/mod.rs`

---

## Review-Credited Findings Incorporated

### Critical blockers (must fix first)
1. **WalkDir currently traverses all of `node_modules`** (Opus, verified in `ModuleResolver::build_module_map_with_diagnostics` in `src/resolver/mod.rs`).
2. **Resolver missing `extension_alias` for NodeNext `.js`→`.ts/.tsx` lookup** (Athena, verified in `build_resolve_options` in `src/resolver/mod.rs`).
3. **Resolver `condition_names` missing `"node"`** (Athena, verified).
4. **Resolver `condition_names` missing `"types"`** (Opus, verified).

### Scope/complexity adjustments
1. **P1.4 first-party classification is simpler than originally estimated** because resolver already runs with `symlinks: true` and classification occurs on resolved path (`src/resolver/mod.rs` + `src/resolver/classify.rs`). Keep task but downgrade complexity and narrow scope.
2. **P1.3 nearest-tsconfig is over-scoped for MVP/OpenClaw** (no nested tsconfigs in OpenClaw extensions/packages; only vendor subtree is special). Defer full nearest-tsconfig to P3.
3. **P2.3 barrel file handling is lower complexity** because parser already emits one file-edge per re-export (`ExportAllDeclaration` / `ExportNamedDeclaration` in `src/parser/mod.rs`) and avoids symbol-level explosion.

### Additional required coverage added
- Type-only imports (`import type`), dynamic imports, and import attributes.
- `vendor/` directory handling.
- Pre-split `src/cli/mod.rs` before Wave B.
- Performance risk + mitigation section.
- Measurable acceptance criteria for each task and for overall readiness.

---

## Pre-Build Prerequisites (P1.0 quick fixes)

These are required before any broader implementation work or OpenClaw dogfooding.

### P1.0a — Exclude heavy directories from module-map walk
**Description**  
Add directory pruning in file discovery to skip `node_modules` at minimum; also include sensible default excludes (`.git`, `dist`, `build`, `target`, `coverage`, `vendor`) and configurable re-include for teams that need vendored code in their module map.

**Files to modify**
- `src/resolver/mod.rs` (`build_module_map_with_diagnostics` WalkDir traversal)
- `src/spec/config.rs` (optional configurable excludes; default list)
- `docs/` (config docs)

**Acceptance criteria (measurable)**
- `node_modules/` is never descended during module-map build (unit test + integration fixture assertion).
- On OpenClaw checkout, module-map build wall-clock for warm cache is reduced by at least **80%** versus baseline before this task.
- No files under excluded dirs appear in module map unless explicitly re-included by config.

**Complexity**: Medium  
**Dependencies**: none  
**Review source**: Opus

---

### P1.0b — Add NodeNext extension alias support
**Description**  
Configure resolver `extension_alias` so `.js` imports can resolve `.ts`/`.tsx` sources under NodeNext patterns.

**Files to modify**
- `src/resolver/mod.rs` (`build_resolve_options`)
- `src/resolver/mod.rs` tests (or new resolver fixture tests)

**Acceptance criteria (measurable)**
- Import `./x.js` from `a.ts` resolves to `x.ts` when `x.js` does not exist.
- Import `./x.js` resolves to `x.tsx` in TSX case.
- Regression tests include a NodeNext-style fixture representative of OpenClaw patterns.

**Complexity**: Low  
**Dependencies**: none  
**Review source**: Athena

---

### P1.0c — Expand resolver condition names
**Description**  
Add `"node"` and `"types"` to resolver `condition_names` and document resolution intent.

**Files to modify**
- `src/resolver/mod.rs` (`build_resolve_options`)
- `docs/` resolver behavior notes
- tests for exports-condition selection

**Acceptance criteria (measurable)**
- Resolver options include conditions in priority order: `import`, `require`, `node`, `types`, `default`. Runtime conditions take precedence; `types` available as fallback for type-declaration-only packages.
- Fixture package with exports conditions verifies selected path changes as expected when `node`/`types` are present.
- `doctor compare` mismatch rate decreases on condition-sensitive fixtures.

**Complexity**: Low-Medium  
**Dependencies**: none  
**Review source**: Athena + Opus

---

## Phase P1 — Foundation (monorepo-aware, performant, resolver-correct)

### P1.1 — Workspace/package discovery (pnpm + package.json workspaces)
**Description**  
Discover first-party workspace roots and package names from `pnpm-workspace.yaml` and `package.json.workspaces`, producing a normalized workspace map.

**Files to modify**
- `src/spec/` (new discovery module)
- `src/spec/config.rs`
- `src/cli/mod.rs` and/or `src/cli/init.rs` wiring

**Acceptance criteria (measurable)**
- Detects OpenClaw-style workspace patterns (`extensions/*`, `packages/*`, root).
- Detects npm/yarn workspace field when pnpm file absent.
- Emits deterministic sorted workspace map (stable snapshot test).

**Complexity**: Medium  
**Dependencies**: P1.0a

---

### P1.2 — Upgrade `specgate init` scaffold generation
**Description**  
Replace single-root heuristics with workspace-aware module scaffolding.

**Files to modify**
- `src/cli/init.rs` (preferred extraction target)
- `src/cli/mod.rs` (thin integration layer)
- tests for init output snapshots

**Acceptance criteria (measurable)**
- On OpenClaw-like fixture, generated modules include root + workspace package modules.
- `--module` / `--module-path` overrides continue to take precedence.
- Output remains deterministic across repeated runs.

**Complexity**: Medium  
**Dependencies**: P1.1

---

### P1.3 — First-party classification policy hardening
**Description**  
Narrowly harden classification rules rather than building a large new model. Keep symlink-realpath behavior as baseline. `vendor/` is excluded from module map by default (handled in P1.0a), so classification doesn't need vendor-specific logic.

**Files to modify**
- `src/resolver/classify.rs`
- `src/resolver/mod.rs` (only minimal plumbing if needed)

**Acceptance criteria (measurable)**
- Workspace symlinked packages resolve/classify as first-party without extra package-map hacks (fixture test).
- No regression: true external `node_modules` dependencies remain third-party.

**Complexity**: Low-Medium (downgraded further — vendor policy handled by exclusion in P1.0a, symlink behavior already correct per Athena's finding)  
**Dependencies**: P1.0a, P1.1

---

### P1.4 — Parser parity hardening for TS/JS import forms
**Description**  
Ensure parser-path correctness for type-only imports, dynamic import forms, and import attributes behavior.

**Files to modify**
- `src/parser/mod.rs`
- parser tests/fixtures under `tests/`
- docs for unsupported dynamic cases (non-literal template imports)

**Acceptance criteria (measurable)**
- `import type` already identified via `is_type_only`; dependency pipeline preserves this flag end-to-end.
- `import()` with string literals produces dynamic import edges; non-literals produce structured warnings (already present) with stable rule code.
- Files using `import ... with { type: "json" }` parse successfully and retain import edge extraction.

**Complexity**: Medium  
**Dependencies**: none (but should land before P2 policy wiring)

---

## Phase P2 — Policy correctness + doctor parity + low-risk graph robustness

### P2.1 — Dependency policy treatment for type-only vs runtime edges
**Description**  
Add explicit policy behavior for type-only imports. Default: type-only imports are excluded from dependency rule checks (they don't create runtime coupling). Opt-in config flag to enforce them for teams wanting conceptual-coupling strictness.

**Files to modify**
- `src/graph/` edge model plumbing (add `is_type_only` to edge metadata)
- `src/rules/` dependency rule evaluation (filter type-only edges by default)
- `src/verdict/` message rendering (label type-only edges distinctly when enforced)
- `src/spec/config.rs` (opt-in toggle: `enforce_type_only_imports: false` default)

**Acceptance criteria (measurable)**
- Type-only imports are represented distinctly in graph edge metadata.
- Default behavior: type-only edges are excluded from `allow_imports_from` and `forbidden_dependencies` checks.
- With `enforce_type_only_imports: true`, type-only edges are enforced identically to runtime edges.
- Snapshot tests show expected verdict differences when toggle changes.

**Complexity**: Medium-High  
**Dependencies**: P1.4

---

### P2.2 — Resolver/TypeScript parity via `doctor compare`
**Description**  
Harden parity diagnostics for alias/exports/condition mismatches and align snapshot schema between npm generator and Rust consumer.

**Files to modify**
- `src/cli/mod.rs` (doctor compare path; later split)
- `npm/specgate/src/generate-resolution-snapshot.js`
- docs: snapshot schema contract

**Acceptance criteria (measurable)**
- Structured snapshot generated from npm wrapper is accepted directly by `doctor compare`.
- On parity fixture suite, mismatch categories include explicit tags (`extension_alias`, `condition_names`, `paths`, `exports`).
- Golden tests lock snapshot schema version and backward compatibility behavior.

**Complexity**: Medium  
**Dependencies**: P1.0b, P1.0c

---

### P2.3 — Barrel/re-export robustness validation
**Description**  
Keep implementation light; parser already models barrel edges at file level. Focus on test coverage and rule interaction correctness.

**Files to modify**
- tests/fixtures for `export *`, named re-exports, and type re-exports
- minimal touches in `src/parser/mod.rs` only if fixture exposes bug

**Acceptance criteria (measurable)**
- `export * from` yields single re-export edge per statement.
- `export { a } from` yields scoped named re-export metadata.
- No edge-count explosion in fixture with layered barrels (edge count bounded by declaration count).

**Complexity**: Low-Medium (downgraded)  
**Dependencies**: none

---

## Phase P3 — Deferred advanced resolver context + distribution polish

### P3.1 — Nearest-tsconfig / multi-context resolver (deferred from MVP)
**Description**  
Implement per-file tsconfig context selection for repos with nested tsconfigs/project refs, while preserving root-tsconfig fast path used by OpenClaw MVP.

**Files to modify**
- `src/resolver/mod.rs`
- potential new resolver context cache module
- integration fixtures with nested tsconfigs

**Acceptance criteria (measurable)**
- Files in nested-tsconfig fixture resolve using their owning tsconfig context.
- OpenClaw-style single-root-tsconfig path remains unchanged and benchmark-neutral (±5%).
- Resolver cache hit rate remains above target threshold (set in perf tests).

**Complexity**: High  
**Dependencies**: P2.2

---

### P3.2 — npm wrapper hardening (`npx specgate check`)
**Description**  
Finalize wrapper UX/release behavior consistent with support matrix.

**Files to modify**
- `npm/specgate/package.json`
- `npm/specgate/src/index.js`
- CI workflow(s)

**Acceptance criteria (measurable)**
- Fresh environment smoke test passes `npx specgate check` on TS fixture repo.
- Release workflow verifies binary fetch/selection across supported OS/arch matrix.

**Complexity**: Medium  
**Dependencies**: P2.2

---

### P3.3 — OpenClaw-scale regression gate
**Description**  
Add end-to-end fixture suite representative of OpenClaw patterns and enforce in CI.

**Files to modify**
- `tests/fixtures/` (multi-package + alias + conditions + dynamic/type-only + barrel)
- CI config

**Acceptance criteria (measurable)**
- CI gate runs full TS/JS parity suite on every PR touching parser/resolver/rules.
- Baseline performance budget enforced (module map + check runtime thresholds).

**Complexity**: Medium  
**Dependencies**: P2.1, P2.2, P2.3

---

## Wave Execution Plan (Zero In-Wave File/Function Conflicts)

### Wave 0 — Prerequisites (serial)
1. P1.0a (`src/resolver/mod.rs` walk filtering)
2. P1.0b (`src/resolver/mod.rs` resolve options)
3. P1.0c (`src/resolver/mod.rs` resolve options/tests/docs)

**Why serial:** all touch `build_resolve_options` and/or module-map traversal.

---

### Wave A — Foundation split (parallel-safe)
- **A1:** P1.1 workspace discovery (owns `src/spec/*`, no resolver edits)
- **A2:** P1.4 parser parity hardening (owns `src/parser/*`, parser fixtures)

**No conflicts:** separate directories and functions.

---

### Wave A′ — Resolver classification follow-on (serial after A1)
- **A3:** P1.3 classification hardening (`src/resolver/classify.rs`, minimal `mod.rs` integration)

**Why not parallel with A2/A1 resolver work:** addresses Opus concern that resolver changes share function-level surface.

---

### Wave B0 — Pre-split monolithic CLI file (required)
- Extract `init` flow from `src/cli/mod.rs` into `src/cli/init.rs`
- Extract `doctor compare` flow into `src/cli/doctor.rs`

**Purpose:** eliminate merge pressure before B1/B2.

---

### Wave B — CLI/domain work (parallel-safe after B0)
- **B1:** P1.2 init scaffold upgrade (primarily `src/cli/init.rs`)
- **B2:** P2.2 doctor/snapshot alignment (primarily `src/cli/doctor.rs` + npm wrapper)
- **B3:** P2.3 barrel robustness fixtures (tests-only lane)

**No conflicts:** function/file ownership separated by B0 split.

---

### Wave C — Policy + finalization
- **C1:** P2.1 type-only policy wiring (graph/rules/verdict)
- **C2:** P3.2 npm wrapper polish
- **C3:** P3.3 e2e regression gate

**Constraint:** C1 should land before final CI-gate lock.

---

### Wave D — Deferred advanced resolver
- **D1:** P3.1 nearest-tsconfig multi-context resolver

**Reason:** high complexity, not required for OpenClaw MVP readiness.

---

## Performance Risks and Mitigations

1. **File discovery explosion** (highest risk)  
   - Risk: full tree walk through `node_modules`, vendor/build artifacts.  
   - Mitigation: directory pruning + configurable excludes + perf budget test.

2. **Resolver context churn (future P3.1)**  
   - Risk: per-file tsconfig resolver instantiation overhead.  
   - Mitigation: tsconfig-context cache keyed by canonical tsconfig path.

3. **Graph noise from type-only edges**  
   - Risk: false-positive architectural violations.  
   - Mitigation: typed edge class + policy switch + explicit verdict labels.

4. **Monolithic CLI merge conflicts**  
   - Risk: parallel work blocked by `src/cli/mod.rs` churn.  
   - Mitigation: mandatory B0 pre-split.

---

## Measurable Overall Exit Criteria (MVP readiness)

Specgate is TS/JS ecosystem MVP-ready when all conditions are true:
1. `specgate init && specgate check` runs on OpenClaw-scale fixture without manual source edits and with **≤2 config tweaks** post-init.
2. NodeNext `.js` import suffix patterns in TS resolve successfully via extension alias tests.
3. Resolver condition parity tests pass for `node` and `types` exports conditions.
4. Module map build excludes `node_modules` and meets performance budget (documented baseline threshold).
5. Parser handles type-only imports, import attributes, and dynamic imports (literal + unresolved-warning modes).
6. CI includes TS/JS regression fixture suite and fails on parity/perf regressions.

---

## Decisions (Resolved 2026-03-03, Trey + Lumen)

1. **Type-only imports policy default → (A) Ignore by default, opt-in enforcement.**  
   Type-only imports (`import type`) don't create runtime coupling. They vanish at compile time, don't affect bundle size, can't cause circular dependency crashes, and don't create deployment dependencies. Default: excluded from dependency rule checks. Parser already tracks `is_type_only`; configurable enforcement is cheap to add later for teams wanting conceptual-coupling strictness.

2. **`vendor/` default classification → (A) Exclude from module map by default.**  
   Vendored code is functionally third-party. Including it creates noise (violations in code you don't control). Default: `vendor/` added to excluded directories alongside `node_modules/`, `dist/`, `build/`, etc. Teams with tightly-integrated vendor forks can explicitly re-include via config.

3. **`types` condition priority → Always include, lower priority than runtime conditions.**  
   Resolver condition order: `import`, `require`, `node`, `types`, `default`. Runtime-style resolution wins for source files; `types` available as fallback for packages that only expose types via that condition. Avoids resolving to `.d.ts` over real source while maintaining accuracy for type-declaration-only packages.

4. **P3.1 nearest-tsconfig timing → Defer to post-MVP.**  
   OpenClaw uses a single root tsconfig. The only nested tsconfig is in `vendor/`, which is excluded by decision #2. No target repo currently requires per-file tsconfig context. Ship MVP, dogfood, add nearest-tsconfig when a real repo needs it.

---

## Final Sequenced Task Order

0. P1.0a node_modules/exclude pruning  
1. P1.0b extension_alias  
2. P1.0c condition_names (`node`, `types`)  
3. P1.1 workspace discovery  
4. P1.4 parser parity hardening  
5. P1.3 classification hardening + vendor policy  
6. Wave B0 CLI pre-split (`mod.rs` extraction)  
7. P1.2 init scaffold upgrade  
8. P2.2 doctor/snapshot parity  
9. P2.3 barrel robustness validation  
10. P2.1 type-only dependency policy  
11. P3.2 npm wrapper hardening  
12. P3.3 e2e OpenClaw-scale gate  
13. P3.1 nearest-tsconfig multi-context resolver (deferred advanced)

---

This plan preserves the original P1/P2/P3 structure, incorporates both reviews’ critical findings, verifies source-level feasibility claims, and reorders work to remove immediate blockers and parallel-merge hazards.