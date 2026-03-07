# TS/JS Ecosystem Support Implementation Plan (Specgate)

Goal: Make Specgate reliably usable on real TypeScript/JavaScript monorepos (OpenClaw-class), including accurate resolution, practical module discovery, and frictionless npm distribution.

Date: 2026-03-03
Branch baseline reviewed: `master`
Comparison branch reviewed: `origin/codex/tsjs-v1-scale-foundation`

---

## 0) Grounded current-state summary (with evidence)

### Resolver today
- Resolver is already based on `oxc_resolver` with TS config auto-wiring only for a root `tsconfig.json` (`build_resolve_options` checks `project_root/tsconfig.json` only): `src/resolver/mod.rs:312-321`.
- Resolver extensions/conditions are set for TS/JS + Node-style conditions and `node_modules`: `src/resolver/mod.rs:323-339`.
- First-party vs third-party classification is currently path-based (`node_modules` path component => third-party; otherwise inside project root => first-party): `src/resolver/classify.rs:83-99`, `src/resolver/classify.rs:142-147`.
- Module ownership map is built by glob matching all files under project root; module overlap diagnostics exist and precedence is deterministic: `src/resolver/mod.rs:163-247`.
- Glob matching uses `literal_separator(true)` to avoid `*` crossing directories: `src/resolver/mod.rs:193-200`.

### Spec model today
- Spec schema supports `version` 2.2 and 2.3, with `CURRENT_SPEC_VERSION` 2.3 and `Boundaries.contracts` already present: `src/spec/types.rs:37-55`, `src/spec/types.rs:49`, `src/spec/types.rs:163-165`.
- `SpecFile` includes `package`, `import_id`, and `import_ids`, which are relevant for TS/JS module/import identity: `src/spec/types.rs:64-73`.

### CLI flow today
- `check` pipeline is `load_project -> analyze_project -> classify/baseline -> verdict`: `src/cli/mod.rs:444-696`.
- `analyze_project` constructs `ModuleResolver` and `DependencyGraph`, then boundary/dependency rules: `src/cli/mod.rs:2759-2834`.
- `init` scaffolding is directory-heuristic only (`src`/`src/app`/common root dirs), not workspace/package.json-aware: `src/cli/mod.rs:1115-1180`.
- `doctor compare` exists and has focused import parity path; parser mode and snapshot in/out flags already exist: `src/cli/mod.rs:163-193`, `src/cli/mod.rs:1421-1662`.

### Existing roadmap and contract docs
- v1.1 implementation plan already includes resolver parity command and MVP statement, but TS/JS-at-scale module discovery is not concretely planned: `docs/specgate-implementation-plan-v1.1.md:18-20`, `docs/specgate-implementation-plan-v1.1.md:209-225`, `docs/specgate-implementation-plan-v1.1.md:64-66`.
- Support matrix already defines npm wrapper + release gating expectations: `docs/support-matrix-v1.md:3`, `docs/support-matrix-v1.md:13-15`, `docs/support-matrix-v1.md:33-42`.
- Boundary contracts doc explicitly reinforces deterministic glob semantics and TS/JS dogfooding lessons: `docs/specgate-boundary-contracts-v2.md:12-19`, `docs/specgate-boundary-contracts-v2.md:32`.

### Target project (OpenClaw) needs
- OpenClaw is a pnpm workspace repo (`pnpm-workspace.yaml`) with root + `ui` + `packages/*` + `extensions/*`: `/Users/treygoff/Development/openclaw/pnpm-workspace.yaml:1-5`.
- Root TS config uses `moduleResolution: NodeNext` and declares `paths` aliases (`openclaw/plugin-sdk`, etc.): `/Users/treygoff/Development/openclaw/tsconfig.json:10-24`.
- Imports show mixed patterns needed by Specgate:
  - Relative imports with explicit `.js` suffix from TS source: grep sample includes `./pairing-messages.js`: `/Users/treygoff/Development/openclaw/src/pairing/pairing-challenge.ts:1`.
  - Node builtins via `node:` prefix and bare forms: grep sample from pairing/store tests: `/Users/treygoff/Development/openclaw/src/pairing/pairing-store.test.ts` sample output.
  - Workspace alias imports (`openclaw/plugin-sdk`) heavily used in `extensions/*`: grep findings under `extensions/...`.
- Repository structure includes `src`, `extensions`, `packages`, `ui`, and `apps`, indicating multi-root first-party boundaries: `/Users/treygoff/Development/openclaw` top-level listing.

### Stale branch (`origin/codex/tsjs-v1-scale-foundation`) assessment
- Diff scope is small and mostly behind master:
  - `npm/specgate/package.json`
  - `npm/specgate/src/generate-resolution-snapshot.js`
  - `src/resolver/mod.rs`
  - `src/spec/config.rs`
- The stale branch removes `literal_separator(true)` usage in resolver globs and deletes tests that validate it; this conflicts with the Hearth-learned glob requirement in contracts v2 and current master behavior:
  - stale diff in `src/resolver/mod.rs` (GlobBuilder -> Glob; tests removed)
  - requirement in `docs/specgate-boundary-contracts-v2.md:32`
  - current master code in `src/resolver/mod.rs:193-200`, tests at `src/resolver/mod.rs:457-499`
- The stale branch changes telemetry config shape to object-only and drops boolean compatibility (`src/spec/config.rs`), conflicting with current backward-compatible behavior on master (`deserialize_telemetry`): current master `src/spec/config.rs:30-36`, `82-101`.
- NPM wrapper changes are minor (runCli argv defaulting + script tweak). Snapshot generator infrastructure itself already exists on master and is reusable as-is foundation: `npm/specgate/src/generate-resolution-snapshot.js:389-467`, `npm/specgate/package.json:15-36`.

Conclusion: salvage ideas, not direct cherry-picks, except potentially tiny npm wrapper ergonomics.

---

## 1) Definition of “works on real JS/TS projects” (acceptance bar)

Specgate is considered TS/JS-ecosystem ready when all are true:
1. `specgate init` can discover module candidates from monorepo workspace manifests and common source roots, not just `src` heuristics.
2. Resolver parity for common TS/JS patterns is high-confidence:
   - relative imports with extension swapping (`.js` in source -> `.ts/.tsx`),
   - package `exports` conditions,
   - `tsconfig` path aliases,
   - barrel re-export traversal compatibility at file-edge level,
   - stable first-party vs third-party classification for workspace packages and `node_modules`.
3. `specgate check` emits useful violations on OpenClaw-like layouts without requiring brittle manual config surgery.
4. `npx specgate check` works from npm wrapper with predictable platform behavior aligned to support tiers.
5. `doctor compare` can consume resolution snapshots that mirror TS compiler behavior for focused mismatches.

---

## 2) Phased implementation plan

## P1 — Foundation (monorepo awareness + resolver correctness)

### Task P1.1: Add workspace/package-aware project metadata discovery
- Description: Introduce a discovery component that reads root `package.json`, `pnpm-workspace.yaml`, and child package manifests to enumerate first-party package roots and package names.
- Files to modify:
  - `src/spec/config.rs` (new config knobs for discovery hints/overrides)
  - `src/cli/init.rs` and/or `src/cli/mod.rs` (init integration)
  - `src/spec/` (new module for workspace discovery)
- Acceptance criteria:
  - Discovery identifies roots from pnpm workspaces (patterns like `packages/*`, `extensions/*`, plus `.`).
  - Detection gracefully handles absent workspace file.
  - Unit tests cover pnpm-style workspace glob expansion.
- Complexity: Medium
- Dependencies: none

### Task P1.2: Upgrade `specgate init` scaffold inference for TS/JS repos
- Description: Replace/augment current `infer_init_scaffold_specs` heuristics with workspace-aware module generation and better path defaults.
- Files to modify:
  - `src/cli/mod.rs` (`infer_init_scaffold_specs` currently at `1115-1180`)
  - `src/cli/init.rs` (if split ownership)
- Acceptance criteria:
  - On OpenClaw-like repo, generated scaffold includes modules for workspace packages (root/app + extensions/packages/ui where applicable), not only one `app` module.
  - Existing `--module` and `--module-path` overrides retain precedence.
  - JSON output includes deterministic created/skipped lists as before.
- Complexity: Medium
- Dependencies: P1.1

### Task P1.3: Resolver config loading from nearest/explicit tsconfig (not root-only)
- Description: Expand resolver option builder to support nearest tsconfig per importing file (or project reference map), while preserving deterministic behavior.
- Files to modify:
  - `src/resolver/mod.rs` (currently root-only tsconfig at `312-321`)
  - `src/cli/mod.rs` (plumbing for resolver context if needed)
- Acceptance criteria:
  - Imports in nested workspace package with local tsconfig resolve consistently.
  - Root-only behavior remains fallback.
  - Tests for root tsconfig + nested tsconfig scenarios.
- Complexity: High
- Dependencies: P1.1

### Task P1.4: First-party package classification model (workspace package map)
- Description: Add explicit first-party package-name map to resolve cases where package specifier points to a workspace package symlinked under `node_modules`.
- Files to modify:
  - `src/resolver/classify.rs`
  - `src/resolver/mod.rs`
  - potentially `src/spec/types.rs` usage for `package` field
- Acceptance criteria:
  - `openclaw/plugin-sdk` (alias/workspace) can be classified as first-party when it resolves to workspace code.
  - Third-party classification remains correct for true external deps.
  - No regression on builtins (`node:` and bare builtins).
- Complexity: High
- Dependencies: P1.1, P1.3

---

## P2 — Full support (aliasing, node_modules boundary semantics, doctor parity)

### Task P2.1: Implement tsconfig paths/alias parity hardening
- Description: Validate and harden alias resolution against TypeScript behavior, including NodeNext conditions and path mapping edge cases.
- Files to modify:
  - `src/resolver/mod.rs`
  - `src/cli/mod.rs` (doctor compare mismatch guidance / parser mode flow)
  - tests in resolver/cli suites
- Acceptance criteria:
  - `doctor compare --from ... --import ...` matches TS snapshot for representative alias imports.
  - Mismatch diagnostics explicitly indicate alias/exports/conditions root causes when divergent.
- Complexity: High
- Dependencies: P1.3

### Task P2.2: Node modules boundary handling (first-party vs third-party) policy wiring
- Description: Wire classification outcomes into dependency policy output with clearer semantics for workspace-linked packages vs external dependencies.
- Files to modify:
  - `src/resolver/classify.rs`
  - `src/rules/` dependency rule implementation
  - `src/verdict/` messaging fields if needed
- Acceptance criteria:
  - Violations for forbidden deps reference package identity accurately.
  - Workspace-linked packages are not falsely flagged as external dependencies.
- Complexity: Medium-High
- Dependencies: P1.4

### Task P2.3: Barrel/re-export import-pattern robustness checks
- Description: Ensure file-edge graph behavior around `index.ts` re-exports and export-forwarding remains predictable for policy checks.
- Files to modify:
  - `src/parser/mod.rs`
  - `src/graph/` edge builder modules
  - test fixtures in `tests/`
- Acceptance criteria:
  - Re-export-heavy packages still produce stable, expected edges.
  - No explosion in false positives from barrel files.
- Complexity: Medium
- Dependencies: P1.3

### Task P2.4: Resolution snapshot schema and generation alignment
- Description: Formalize/lock snapshot schema consumed by `doctor compare`, and ensure npm snapshot generator and Rust-side comparator stay in sync.
- Files to modify:
  - `npm/specgate/src/generate-resolution-snapshot.js`
  - `src/cli/mod.rs` (`doctor compare` structured ingestion/output)
  - docs for snapshot schema contract
- Acceptance criteria:
  - Snapshot generated by wrapper is accepted by `doctor compare --structured-snapshot-in` without manual edits.
  - Golden fixture tests cover snapshot contract versions.
- Complexity: Medium
- Dependencies: P2.1

---

## P3 — Polish & distribution

### Task P3.1: npm wrapper distribution hardening for `npx specgate check`
- Description: Finalize wrapper UX, binary download/selection path, and publish/verify automation consistent with support matrix tiers.
- Files to modify:
  - `npm/specgate/package.json`
  - `npm/specgate/src/index.js` / `bin/specgate.js`
  - `.github/workflows/release-npm-wrapper.yml` (already referenced by support matrix)
- Acceptance criteria:
  - Fresh environment can run `npx specgate check` successfully on sample TS repo.
  - Dist-tag verification and smoke checks run in CI.
- Complexity: Medium
- Dependencies: P2.x complete

### Task P3.2: `specgate init` UX polish and docs for JS/TS monorepos
- Description: Improve generated spec comments/examples and user-facing docs for workspace + alias workflows.
- Files to modify:
  - `src/cli/mod.rs` scaffold content
  - docs (`docs/`)
- Acceptance criteria:
  - Docs include monorepo init examples, path alias guidance, and doctor compare workflow.
- Complexity: Low-Medium
- Dependencies: P1.2, P2.1

### Task P3.3: End-to-end dogfood gate on OpenClaw-like fixture
- Description: Add integration fixture(s) representing multi-package + tsconfig paths + alias imports.
- Files to modify:
  - `tests/` integration/golden suites
  - fixture directories under `tests/fixtures/`
- Acceptance criteria:
  - CI includes stable TS/JS parity gate for regression prevention.
- Complexity: Medium
- Dependencies: P2.1, P2.2, P2.3

---

## 3) What to salvage from stale branch vs fresh implementation

### Reuse candidates (low risk)
1. Minor npm wrapper ergonomics in `generate-resolution-snapshot.js` (`runCli` argv defaulting) may be reused if desired for CLI UX consistency.
2. Existing npm wrapper/snapshot architecture itself is already present on master; continue building on master’s version, not stale branch snapshots.

### Do NOT reuse directly
1. Resolver glob regression from stale branch (removing `literal_separator(true)`) must not be adopted.
2. Telemetry config shape change in stale branch should not be cherry-picked; master currently preserves backward compatibility and is integrated in CLI checks.

### Net recommendation
- Re-implement TS/JS scale features fresh on top of current master and contracts-v2 semantics.
- Use stale branch only as historical context; avoid direct cherry-pick except tiny npm wrapper niceties after review.

---

## 4) Parallel wave execution plan (with file conflict flags)

## Wave A (can run in parallel after kickoff)
- A1: Workspace discovery core (new module under `src/spec/` + tests)
- A2: Resolver tsconfig strategy refactor (`src/resolver/mod.rs`) **[conflicts with A3 if same file]**
- A3: Classification model update (`src/resolver/classify.rs`, resolver plumbing)

Conflict notes:
- A2/A3 both touch resolver internals; either sequence them or isolate shared edit windows.

## Wave B (depends on Wave A outputs)
- B1: `specgate init` upgrade (`src/cli/mod.rs` / `src/cli/init.rs`)
- B2: doctor compare + snapshot schema alignment (`src/cli/mod.rs`, npm snapshot generator)

Conflict notes:
- B1/B2 both may touch `src/cli/mod.rs`; coordinate via feature flags or branch stacking.

## Wave C (polish/distribution)
- C1: npm publish/verify workflow hardening
- C2: docs + fixture-based e2e gates

Conflict notes:
- Low code conflict; mostly CI/docs/fixtures.

---

## 5) Risks and open questions

1. **Tsconfig project references at scale**
   - Risk: nearest-tsconfig heuristics may diverge from TypeScript program graph semantics in complex repo references.
   - Mitigation: formalize resolver context selection and validate with `doctor compare` fixtures.

2. **Workspace package classification ambiguity**
   - Risk: workspace package may appear through symlinked `node_modules` path and be misclassified third-party.
   - Mitigation: explicit first-party package-name map from workspace manifests + realpath checks.

3. **Barrel/re-export noise**
   - Risk: edge expansion from barrels can inflate false positives.
   - Mitigation: keep file-edge deterministic model; add targeted fixtures before introducing deeper symbol tracking.

4. **`init` over-generation in monorepos**
   - Risk: generating too many starter modules harms usability.
   - Mitigation: include ranking/filtering heuristics and optional `--init-scope` controls.

5. **NPM wrapper platform drift**
   - Risk: wrapper behavior diverges from binary release matrix and dist-tags.
   - Mitigation: gate wrapper publish verification in CI as required by support matrix.

Open questions to resolve before implementation lock:
- Should module discovery default to package-per-workspace, directory-per-root, or hybrid scoring?
- Should `specgate.config.yml` gain explicit `workspace_roots` and `package_map` overrides in v1?
- How much per-file resolver context is acceptable before check-time performance regresses?
- Should `doctor compare` include batch mode for alias regression packs (not just focused pair)?

---

## 6) Suggested execution order (minimal critical path)

1. P1.1 workspace discovery
2. P1.3 tsconfig context resolution
3. P1.4 first/third-party classification update
4. P1.2 init improvements
5. P2.1 alias parity hardening + doctor workflows
6. P2.2 node_modules policy semantics
7. P2.3 barrel robustness
8. P2.4 snapshot schema alignment
9. P3.1 npm distribution hardening
10. P3.2/P3.3 docs + regression fixtures

This order minimizes rework in `src/resolver/*` and `src/cli/mod.rs`, the two highest-conflict surfaces.
