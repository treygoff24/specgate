# Adversarial Testing Catalog

This catalog summarizes behavior validated by `tests/fixtures/adversarial/*` and what to expect from Specgate today.

## What specgate catches today

| Scenario | Rule exercised | Why this is caught |
|---|---|---|
| `cross-layer-shortcut` | `boundary.never_imports` | Direct import edge from `handlers` to `database` violates an explicit deny rule and is reported immediately. |
| `circular-via-re-export` | `no-circular-deps` | The re-export graph still contributes to dependency edges, so the cycle is detected in SCC/circular analysis. |
| `path-traversal` | `boundary.never_imports` | Resolver normalizes `../../../` traversal and resolves the target module before boundary evaluation. |
| `conditional-require` | `boundary.never_imports` | `require(...)` calls in conditionals are still represented in AST edges and therefore remain enforceable by boundary rules. |
| `dynamic-import-evasion` | `boundary.never_imports` | `await import(...)` is parsed into a static module edge, so boundary checks still apply. |
| `aliased-deep-import` | `boundary.never_imports` | `oxc_resolver` consumes `tsconfig` path aliases, allowing alias targets to be resolved correctly before boundary checks. |
| `ownership-overlap` | Doctor ownership diagnostic | Spec overlaps are surfaced by `specgate doctor ownership` when module ownership globs intersect. |

## What specgate cannot catch yet (known gaps)

For each known gap, this lists (1) why it is currently unhandled, (2) whether there is a planned fix, and (3) available mitigations.

### `barrel-re-export-chain`
- **Why:** Boundary graphing does not collapse chained re-export indirection (`A -> B -> C -> D`) into a single effective edge for `never_imports`; only direct imports are considered during that rule path.
- **Will this be fixed?** Planned for future work with priority **P9**.
- **Workaround:** Keep deny constraints on intermediate barrel modules, avoid multi-hop re-export indirection in policy-critical paths, or enforce stricter API boundaries with code review/linting.

### `wildcard-re-export-leak`
- **Why:** `public_api` is enforced on importer → source-file boundaries, not on wildcard export surface propagation, so `export *` can re-export internal members without explicit boundary checks.
- **Will this be fixed?** Planned for future work with priority **P9**.
- **Workaround:** Replace wildcard exports with explicit named re-exports in public modules, and treat `export *` as a review-only exception.

### `deep-third-party-import`
- **Why:** There is currently no policy around allowed package-entry depth (for example `pkg/lib/internal/*`), so deep imports into third-party internals are not modeled as violations.
- **Will this be fixed?** Planned for future work with priority **P9**.
- **Workaround:** Enforce import hygiene via package policy (`exports` constraints, dependency management tooling), and avoid depending on private package internals in code review.

### `test-helper-leak`
- **Why:** Specgate treats test and production codepaths in one shared module graph; it does not yet model separate test/production boundaries.
- **Will this be fixed?** Planned for future work with priority **P9**.
- **Workaround:** Separate test helpers into dedicated packages/paths and run production-only checks where possible.

### `type-import-downgrade`
- **Why:** Boundary evaluation does not track value-vs-type import semantics; all imports are edges by module target, so a type-only symbol changed to value import is not currently distinguished.
- **Will this be fixed?** Not yet scheduled (labeled **Future** in fixtures).
- **Workaround:** Enforce a local convention (`import type` / `type` exports) and add TypeScript or ESLint rules for type-only import discipline.

### `hallucinated-import`
- **Why:** Unresolved imports are currently treated as silent/no-op for edge construction; there is no hard diagnostic path in current edge-classification behavior.
- **Will this be fixed?** Planned for future work with priority **P6** (edge-classification hardening).
- **Workaround:** Run TypeScript resolution/typecheck (`tsc --noEmit`) before or alongside Specgate in CI.

### `orphan-module`
- **Why:** Modules whose spec globs match zero source files do not generate a policy violation in `specgate check`; they surface through ownership diagnostics instead.
- **Will this be fixed?** `specgate doctor ownership` reports orphaned specs today; `strict_ownership: true` plus `strict_ownership_level: warnings` lets teams gate on those findings in CI.
- **Workaround:** Include `specgate doctor ownership` in workflow to catch orphaned spec modules and keep fixture/module inventories explicit.
