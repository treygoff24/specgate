# TS/JS Ecosystem Support — Code Review

**Reviewer:** Lumen (automated code review)  
**Date:** 2026-03-03  
**Scope:** Commits `c12fe27..9291566` (9 implementation commits atop `a48498f` docs commit)  
**Plan reference:** `docs/tsjs-ecosystem-plan-final.md`  
**Verdict:** All tests pass. Feature is solid. Several important findings below.

---

## 1. Correctness

### Extension Alias Mapping

**Rating: Sound**

The `extension_alias` configuration in `build_resolve_options()` (`src/resolver/mod.rs:350-374`) correctly maps `.js → [.ts, .tsx, .js, .jsx]`, `.mjs → [.mts, .mjs]`, `.cjs → [.cts, .cjs]`, and `.jsx → [.tsx, .jsx]`. The ordering is correct — TypeScript source files are tried before their JS counterparts, which matches NodeNext semantics where `.js` in source means "resolve the TS file that compiles to this."

The test `nodenext_extension_alias_resolves_js_specifier_to_ts_file` at `src/resolver/mod.rs:680` validates the happy path.

**Suggestion:** No test covers the `.mjs → .mts` or `.cjs → .cts` paths. These are lower-priority but worth adding for completeness.  
*File: `src/resolver/mod.rs`, tests section*

### Node Modules Exclusion

**Rating: Robust**

`should_skip_module_map_entry()` at `src/resolver/mod.rs:419-435` uses `Path::components()` with `Component::Normal` matching — this is the correct approach for cross-platform safety. It checks each path segment, so nested `node_modules` within workspaces (e.g., `packages/web/node_modules/`) are correctly excluded.

The `filter_entry` approach at line 172 is efficient — WalkDir prunes the entire subtree rather than visiting and discarding.

**Important:** The exclusion list is hardcoded in two places: `should_skip_module_map_entry()` in `src/resolver/mod.rs:428-435` and `should_skip_workspace_dir()` in `src/spec/workspace_discovery.rs:211-220`. The lists don't match — the resolver skips `vendor` but workspace discovery doesn't, and workspace discovery skips `generated` but that's covered in the resolver too. This isn't a bug today (workspace discovery filters different paths for different reasons), but diverging hardcoded lists are a maintenance hazard.  
*Files: `src/resolver/mod.rs:428-435`, `src/spec/workspace_discovery.rs:211-220`*

**Suggestion:** Extract the shared exclusion list into a constant in `src/spec/config.rs` or a shared utility, so both consumers reference the same source of truth.

### Nearest-Tsconfig Resolver

**Rating: Correct with a caveat**

The `nearest_tsconfig_for_dir()` implementation at `src/resolver/mod.rs:388-413` walks up from the containing directory to the project root, finding the nearest `tsconfig.json`. The boundary check (`!parent.starts_with(project_root)`) correctly prevents escaping the project root.

The per-tsconfig resolver cache (`resolvers_by_tsconfig: BTreeMap<PathBuf, oxc_resolver::Resolver>`) at `src/resolver/mod.rs:77` is keyed by canonicalized path, which avoids duplicates from symlinks.

**Important:** `nearest_tsconfig_for_dir` does a filesystem `exists()` check on every call (no caching of tsconfig locations). For large monorepos with deep directory nesting, this means many repeated `stat()` calls walking up directory trees. The resolver result is cached per (file, specifier) pair, but the tsconfig lookup itself is not.  
*File: `src/resolver/mod.rs:388-413`*

**Suggestion:** Consider caching the `dir → tsconfig_path` mapping in a `BTreeMap<PathBuf, Option<PathBuf>>` alongside the resolver cache.

### Importer Path Canonicalization

**Rating: Good fix**

Commit `9291566` canonicalizes the importer path in `containing_dir()` at `src/resolver/mod.rs:441-451`. This fixes tsconfig lookup for symlinked files — without canonicalization, the nearest-tsconfig walk would search from the symlink location rather than the real location, potentially selecting the wrong tsconfig context.

**Suggestion:** The `unwrap_or(absolute_from_file)` fallback at line 446 silently degrades if canonicalization fails (e.g., broken symlink). Consider logging a diagnostic warning when this happens, since it could cause subtle resolution mismatches.  
*File: `src/resolver/mod.rs:446`*

---

## 2. Completeness vs Plan

### Implemented (verified against plan tasks)

| Task | Status | Notes |
|------|--------|-------|
| **P1.0a** — node_modules/exclude pruning | ✅ Done | `filter_entry` + `should_skip_module_map_entry` |
| **P1.0b** — extension_alias | ✅ Done | All four alias groups configured |
| **P1.0c** — condition_names | ✅ Done | `node` and `types` added in correct priority order |
| **P1.1** — Workspace discovery | ✅ Done | pnpm + package.json workspaces; `workspace_discovery.rs` |
| **P1.2** — Init scaffold upgrade | ✅ Done | Workspace-aware init in `infer_init_scaffold_specs` |
| **P1.4** — Parser parity (import attributes) | ✅ Done | Test added for `with { type: "json" }` |
| **P2.1** — Type-only dependency policy | ✅ Done | Both dependency + boundary rules gated |
| **P2.2** — Doctor mismatch categories | ✅ Done | `extension_alias`, `condition_names`, `paths`, `exports` tags |
| **P2.3** — Barrel robustness | ✅ Done | 4 barrel fixture tests + layered edge-count bounds |
| **B0** — CLI pre-split | ✅ Done | `doctor.rs` extracted cleanly |
| **P3.3** — E2E regression gate | ✅ Done | `tsjs_openclaw_regression.rs` + CI gate |

### Not Implemented (per plan, correctly deferred)

| Task | Status | Notes |
|------|--------|-------|
| **P1.3** — Classification hardening | ⚠️ Not landed | Plan says depends on P1.0a + P1.1, but no explicit classification changes in this merge. The plan downgraded this to "low-medium" and noted existing symlink behavior is correct. Acceptable omission if symlink classification is already tested elsewhere. |
| **P3.1** — Nearest-tsconfig multi-context | ⚠️ Actually landed | The plan explicitly deferred this to post-MVP ("Wave D"), but `ad45e13` implements it. This is ahead of plan, which is fine, but the plan's reasoning was that it's "high complexity, not required for OpenClaw MVP readiness." Review the implementation quality carefully (see §1 above). |
| **P3.2** — npm wrapper hardening | ❌ Not landed | No changes to `npm/specgate/` in this merge. Still pending. |

**Important:** P3.1 (nearest-tsconfig) landed despite being explicitly deferred in the plan. The implementation looks correct and well-tested, but this was categorized as "High complexity" in the plan. If it introduces any issues, it's worth knowing it wasn't planned for this phase.  
*Commit: `ad45e13`*

### Plan-specified acceptance criteria check

**Important:** P1.0a acceptance criteria specified "configurable re-include for teams that need vendored code in their module map." The implementation adds `vendor` to the hardcoded exclusion list (`src/resolver/mod.rs:434`, `src/spec/config.rs:118`) but does **not** add any config-level mechanism to re-include excluded directories. The existing `exclude` config field controls glob-level exclusion, but there's no `include` or `re-include` override.  
*Files: `src/resolver/mod.rs:419-435`, `src/spec/config.rs`*

---

## 3. Test Coverage

### New Test Suites

- **`tests/tsjs_barrel_fixtures.rs`** (102 lines, 4 tests): Covers `export *`, named re-exports, type re-exports, and layered barrel edge-count bounds. Well-structured.
- **`tests/tsjs_openclaw_regression.rs`** (174 lines, 3 tests): Covers init scaffold generation, check determinism, and doctor compare parity. Uses a realistic OpenClaw-scale fixture.
- **`tests/wave2c_cli_integration.rs`** additions (138 lines, 3 tests): Mismatch category classification for paths, exports, and condition_names.
- **`tests/integration.rs`** additions (72 lines, 2 tests): Workspace detection from pnpm and package.json.
- **Parser test** in `src/parser/mod.rs` (35 lines, 1 test): Import attributes parity.
- **Resolver tests** in `src/resolver/mod.rs` (~195 lines, 5 tests): Module map pruning, extension alias, condition_names, nearest-tsconfig, per-file resolver context.
- **Config tests** in `src/spec/config.rs`: Toggle parsing and serialization.
- **Dependency rule tests** in `src/rules/dependencies.rs` (65 lines, 2 tests): Type-only ignore/enforce.
- **Workspace discovery tests** in `src/spec/workspace_discovery.rs` (2 tests): pnpm and package.json fallback.

### Missing Coverage

**Important:** No test for `should_skip_workspace_dir` with nested excluded directories (e.g., a workspace pattern that matches inside `node_modules`). The function filters correctly, but there's no regression test proving it.  
*File: `src/spec/workspace_discovery.rs:207-221`*

**Important:** No test for workspace discovery with duplicate module names (disambiguation via `__` separator). The code at `src/spec/workspace_discovery.rs:55-67` handles this, but there's no test exercising the `extensions__shared` style name collision path.

**Suggestion:** No test for the `mjs → mts` or `cjs → cts` extension alias paths.

**Suggestion:** No negative test for `nearest_tsconfig_for_dir` when `containing_dir` is outside `project_root` — the code handles it (line 393-395) but there's no test.

**Suggestion:** The OpenClaw regression fixture (`tests/fixtures/openclaw-scale/seed/`) doesn't include a `vendor/` directory to verify vendor exclusion end-to-end through the full check pipeline.

**Suggestion:** No test for boundary rule type-only filtering — only dependency rules have explicit tests. The boundary change at `src/rules/boundary.rs:77-79` is simple and correct, but untested in isolation.

---

## 4. Architecture

### Workspace Discovery Module

**Rating: Well-designed**

`src/spec/workspace_discovery.rs` is self-contained with clear responsibility boundaries:
- Pure function `discover_workspace_packages(project_root) → Vec<WorkspacePackage>` — no side effects, deterministic output (sorted).
- Clean separation: `workspace_patterns()` → `expand_workspace_pattern()` → filtering → packaging.
- Uses `BTreeSet` for deterministic ordering throughout.
- The `WorkspacePackage` struct is minimal and derives the right traits (`Eq`, `Ord`, `Clone`).

**Suggestion:** The `normalize_pattern()` function at line 155 strips leading `./` and trailing `/`, which is correct for most cases. But it also `trim_matches('"')` which suggests defensive handling of YAML quoting artifacts. Consider whether this should log a warning when quotes are detected, since it may indicate a malformed `pnpm-workspace.yaml`.

### Doctor Extraction

**Rating: Clean**

The `src/cli/doctor.rs` extraction is a pure move — the code is identical to what was removed from `src/cli/mod.rs`. The module uses `pub(super)` visibility correctly, keeping the handler private to the CLI module. The `use super::*` import is pragmatic for an extraction (avoids churn in import paths) though it couples the module to the parent's namespace.

**Suggestion:** Consider replacing `use super::*` with explicit imports in a follow-up cleanup. This makes the module's actual dependencies visible and prevents accidental coupling.  
*File: `src/cli/doctor.rs:1-3`*

### Init Scaffold Upgrade

**Rating: Good**

The workspace integration in `infer_init_scaffold_specs()` at `src/cli/mod.rs:1127-1155` follows a clean fallback chain: workspace discovery → existing heuristics. The `infer_root_module_path` extraction (returning `Option<String>` instead of always defaulting) is a good refactor that enables the workspace path to produce a root module only when there's actually a root `src/` directory.

**Important:** When workspace packages are discovered but `infer_root_module_path` returns `None` (no `src/` or `src/app/` at root, and no common root dirs), the scaffolds list contains only workspace packages with no root module. This is correct behavior — some monorepos have no root source — but it's a behavioral change from the pre-workspace path where a root module was always generated.  
*File: `src/cli/mod.rs:1133-1140`*

---

## 5. Security / Safety

### Path Traversal

**Rating: No issues found**

- `nearest_tsconfig_for_dir()` bounds the walk to `project_root` (line 402: `!parent.starts_with(project_root)`).
- `expand_workspace_pattern()` walks from `search_root` derived from `project_root.join(prefix)` and only accepts entries that `strip_prefix(project_root)` succeeds on (line 196).
- `should_skip_module_map_entry()` uses `strip_prefix(project_root)` and returns `false` (don't skip) if stripping fails, which is safe — unknown paths outside the project won't match the exclusion list but also won't be in the module map since WalkDir starts at `project_root`.

### Glob Injection

**Rating: Safe**

Workspace patterns from `pnpm-workspace.yaml` and `package.json` are passed through `GlobBuilder::new()` with `literal_separator(true)`, which prevents `*` from matching `/`. The `normalize_pattern()` function strips leading `./` and `\\` normalization but doesn't sanitize glob metacharacters — however, these patterns come from project configuration files (trusted input), not user-supplied runtime input.

### Unsafe Code

**Rating: None**

No `unsafe` blocks in any new or modified code. All `unwrap()` calls in non-test code are `unwrap_or` / `unwrap_or_else` variants with sensible fallbacks.

---

## 6. Performance

### Module Map Exclusion

**Rating: Efficient**

The `filter_entry` callback at `src/resolver/mod.rs:172` prunes entire subtrees, which is the optimal WalkDir pattern. WalkDir won't descend into `node_modules/`, `dist/`, etc., so the performance improvement is proportional to the size of excluded trees — exactly what P1.0a targeted.

### Workspace Discovery

**Rating: Acceptable**

`expand_workspace_pattern()` at `src/spec/workspace_discovery.rs:168-206` uses WalkDir with `max_depth(1)` for non-recursive patterns (e.g., `packages/*`), which is O(entries in parent directory). For `**` patterns, it walks the full subtree — but these are rare in practice and bounded by the search root prefix extraction.

No O(n²) patterns detected. The `BTreeSet` for candidates provides O(n log n) insertion and deduplication.

### Nearest-Tsconfig Lookups

**Important:** As noted in §1, `nearest_tsconfig_for_dir()` is called on every uncached resolution and performs filesystem `stat()` calls walking up the directory tree. For a monorepo with 1000 files across 50 directories, this could mean ~5000 extra `stat()` calls (50 dirs × ~5 levels avg × 20 files per dir that share the same dir). The resolver cache mitigates this for repeated (file, specifier) pairs, but first-pass resolution will pay the cost.  
*File: `src/resolver/mod.rs:388-413`*

**Suggestion:** Add a `tsconfig_cache: BTreeMap<PathBuf, Option<PathBuf>>` field to `ModuleResolver` and check it before walking the filesystem. This is a single-field addition with high ROI for large repos.

---

## 7. Backward Compatibility

### Behavioral Changes

**Important:** The `default_excludes()` in `src/spec/config.rs:116-119` adds three new patterns: `**/target/**`, `**/coverage/**`, `**/vendor/**`. For existing users who have a `specgate.yml` without an explicit `exclude` field, these new defaults will take effect silently. Any user who has source files in directories named `target/`, `coverage/`, or `vendor/` that were previously included in the module map will see those files disappear from analysis results.

This is unlikely to affect most users (these are conventional artifact/output directories), but `vendor/` is the most likely to contain intentionally-analyzed code. The plan acknowledged this (Decision #2: "Teams with tightly-integrated vendor forks can explicitly re-include via config"), but the re-include mechanism doesn't exist yet (see §2).  
*File: `src/spec/config.rs:116-119`*

**Important:** The `containing_dir()` function now canonicalizes the importer path (commit `9291566`). This means resolution lookups are keyed by the canonical (symlink-resolved) path rather than the original path. For projects using symlinks in their source tree, this could change which tsconfig context is selected, potentially altering resolution results. The change is correct in principle (resolve from the real location), but it's a semantic change for existing users with symlinked source trees.  
*File: `src/resolver/mod.rs:446`*

**Suggestion:** Both behavioral changes should be mentioned in release notes for the version that includes this feature.

### Existing Tests

All 22 existing test suites pass. The CI gate (`scripts/ci/mvp_gate.sh`) has been extended to include the two new test suites. No existing test output changed.

The `resolve_uncached` method signature changed from `&self` to `&mut self` (line 273), which is a breaking change for any external code calling this method — but it's `fn` (private), so no public API impact.

---

## Summary of Findings

### Critical
*None.*

### Important (7)
1. **Diverging exclusion lists** between `should_skip_module_map_entry` and `should_skip_workspace_dir` — maintenance hazard. (`src/resolver/mod.rs:428`, `src/spec/workspace_discovery.rs:211`)
2. **Nearest-tsconfig lookup not cached** — repeated filesystem walks on every uncached resolution. (`src/resolver/mod.rs:388-413`)
3. **No configurable re-include for excluded directories** — plan acceptance criteria not met. (`src/spec/config.rs`)
4. **P3.1 nearest-tsconfig landed despite being deferred in plan** — implementation looks correct but was "High complexity" and explicitly deferred. (Commit `ad45e13`)
5. **No test for workspace module name disambiguation** — `__` separator path untested. (`src/spec/workspace_discovery.rs:55-67`)
6. **New default excludes (`target`, `coverage`, `vendor`) could silently change behavior** for existing users. (`src/spec/config.rs:116-119`)
7. **Importer path canonicalization is a semantic change** for symlinked source trees. (`src/resolver/mod.rs:446`)

### Suggestion (8)
1. Add `.mjs → .mts` and `.cjs → .cts` extension alias tests.
2. Log diagnostic when `fs::canonicalize` fails in `containing_dir()`.
3. Extract shared exclusion list into a constant.
4. Replace `use super::*` in `doctor.rs` with explicit imports.
5. Add `vendor/` directory to OpenClaw regression fixture.
6. Add boundary rule type-only filtering test.
7. Test `nearest_tsconfig_for_dir` with out-of-root `containing_dir`.
8. Mention behavioral changes in release notes.
