# Specgate: MVP Specification v2

### Machine-Checkable Architectural Intent for Agent-Generated Code

**Version:** 2.1 — February 25, 2026
**Status:** Ready for build

---

## 1. Problem Statement

### The verification gap

AI coding agents produce syntactically valid, type-correct, test-passing code that is still wrong. Code generation is cheap and fast. Verification remains expensive and manual. This is the dangerous quadrant of the automation-verification axis: easy to automate, hard to verify.

Teams running multi-agent coding workflows hit the same wall. Agents write code at 10x speed. Humans spend 3x longer reviewing it. The net gain evaporates into verification overhead.

### The six failure modes

From extensive experience running 10+ parallel coding agents on production software:

1. **Semantic drift.** The agent implements something subtly different from the spec. It passes every check because no check encodes what was actually intended.

2. **Breaking distant code.** Agent modifies module A, which breaks an implicit contract with module C. No test covers the interaction.

3. **Hallucinated interfaces.** Agent calls functions that don't exist or uses wrong signatures. Type checkers catch some; dynamic languages catch none.

4. **Lost constraints.** The spec says "never do X." Negative constraints are invisible to tests. The agent just does X.

5. **Architectural erosion.** Agent adds a direct database call from the UI layer. Creates a circular dependency. Bypasses auth middleware. Structurally wrong, functionally "works."

6. **Test-implementation collusion.** Same agent writes both test and implementation. The test verifies the code. The code satisfies the test. Neither matches intent. The student grades their own exam.

### Why existing tools fail

| Tool | What it checks | What it misses |
|------|---------------|----------------|
| Type checker | Shape of data | Whether the right data flows to the right place |
| Linter | Code patterns | Whether the logic is correct |
| Unit tests | Specific input→output | Untested paths, emergent interactions |
| Integration tests | Component interactions | Architectural violations, spec conformance |
| E2E tests | User-visible flows | Internal invariants, security properties |
| AI code review | "Does this look right?" | Everything — it guesses, it doesn't prove |

The fundamental gap: **intent is not formally captured anywhere in the development process.**

---

## 2. Core Insight

**Intent is knowable — we wrote it down.** Every task has acceptance criteria, constraints, and architectural boundaries. These exist as natural language today. They can exist as machine-checkable specifications tomorrow.

**The agent that writes the code must never write the verification.** Separation of concerns is the foundation of trustworthy verification.

**We don't need to verify everything — just what agents break most.** The 80/20: most agent-produced defects are structural. Wrong imports. Broken boundaries. Violated invariants. These are concrete, checkable structural properties.

**Deterministic verification is the only way out of the probabilistic trap.** You cannot solve "AI might be wrong" by adding more AI. Formal specs checked mechanically, with zero ambiguity in the verdict.

---

## 3. The Solution: Specgate

Specgate provides machine-checkable architectural intent gates for agent-generated code. Developers express architectural intent as structured YAML specs. The engine deterministically verifies code against those specs using AST analysis — no AI in the verification loop.

### Phase 1 (MVP): Structural Policy Engine

Static analysis of code structure against declared specifications:

- **Module boundary enforcement.** Module A declares it imports from [B, C] and never from [D, E]. The engine parses the AST and confirms or denies. Binary.
- **Public entrypoint enforcement.** Modules declare their public API files. Importing internal files from outside the module is a violation.
- **Third-party dependency boundaries.** Control what npm packages a module can import. Prevent agents from hallucinating packages or importing Node built-ins into browser code.
- **Layer architecture enforcement.** Declare ordered layers. Verify no layer imports from layers above it.
- **Circular dependency detection.** Within and between modules.
- **Type-only import distinction.** Allow `import type` even when runtime imports are forbidden.

Phase 1 catches three of the six failure modes directly: architectural erosion, hallucinated interfaces (wrong imports), and lost constraints (negative boundary rules). It partially addresses breaking distant code through cross-module boundary specs.

### Phase 2: Runtime Invariants (Post-MVP)

- Typed expression language for invariants
- Auto-injected runtime assertions derived from specs
- Explicit bindings layer mapping spec concepts to code constructs

### Phase 3: Behavioral Verification (Post-MVP)

- Typed events and state predicates
- Behavioral baseline recording and snapshot diffing
- Anti-flake tooling: trace normalization, deterministic replay

---

## 4. The Core Engineering Challenge: Module Resolution

**Module resolution is the #1 engineering priority of the MVP.** If resolution is wrong, every boundary check, every import validation, every layer enforcement rule produces garbage results. This is the hardest problem in the engine and must receive proportional engineering time.

### What must be resolved correctly

- **tsconfig `paths` and `baseUrl` aliases.** Projects use `@/components/*`, `@api/*`, etc. The engine must read `tsconfig.json` and resolve these to real file paths before any checking occurs.
- **`index.ts` fallbacks.** `import from './utils'` may resolve to `./utils.ts`, `./utils/index.ts`, or `./utils/index.tsx`. All must be handled.
- **Barrel file re-exports.** `export * from './internal'` chains must be followed to determine the true origin of symbols.
- **Relative imports.** `../../shared/types` must resolve relative to the importing file's location.
- **Monorepo workspace packages.** `import from '@myorg/shared'` may resolve via pnpm/yarn workspace symlinks. The engine must follow symlinks to real paths.
- **Workspace symlinks.** `node_modules/@myorg/pkg` → `../../packages/pkg`. Must resolve through the symlink.

### Resolution strategy

1. Read `tsconfig.json` (and extended configs) to build an alias resolution map.
2. Implement Node/TypeScript module resolution algorithm: check exact path, then `.ts`, `.tsx`, `.js`, `.jsx`, then `/index.*`.
3. For workspace packages, read `package.json` workspaces config and resolve package names to local paths.
4. Cache the resolution map per run. Invalidate on config file changes.
5. The `specgate doctor` command exposes resolution internals for debugging.

### Why this is hard

Edge cases compound. A path alias pointing to a workspace package that re-exports from a barrel file that re-exports from an internal module — the engine must follow the entire chain to determine whether a boundary is violated. One wrong resolution and the engine either misses a real violation or reports a false positive. Either outcome destroys trust.

---

## 5. Spec Language Design

### Schema

```yaml
# specgate.schema: v2
version: "2"

module: string          # Module identifier (maps to directory path)
description: string     # Human-readable description (not verified)

boundaries:
  path: string          # Glob pattern for files in this module (default: ./**/*)
  public_api:           # Public entrypoint files — external modules must import through these only
    - string            # e.g., "index.ts"
  allow_imports_from:   # Allowed import sources (DEFAULT-DENY when defined)
    - string            # Module names or glob patterns
  never_imports:        # Forbidden import sources (hard deny, overrides everything)
    - string
  allow_type_imports_from:  # Modules allowed for type-only imports even when runtime is forbidden
    - string
  allowed_dependencies:     # Permitted third-party npm packages
    - string
  forbidden_dependencies:   # Banned third-party npm packages (hard deny)
    - string

constraints:            # Architectural rules
  - rule: string        # Rule identifier
    params: object      # Rule-specific parameters
    severity: string    # "error" (default) or "warning"
    message: string     # Human-readable explanation

# Phase 2+ (documented, not enforced in MVP)
invariants: [...]
behavior: [...]
```

### Import resolution semantics

**Default-deny mode.** When `allow_imports_from` is defined on a module, that module is in default-deny mode. Only imports from explicitly listed modules are permitted. Any unlisted import source is a violation.

**Default-allow mode.** When `allow_imports_from` is omitted, any import is allowed unless it appears in `never_imports`.

**Precedence: deny > allow > implicit default.**
1. `never_imports` is checked first. If a source matches, it's a violation regardless of other rules.
2. `allow_imports_from` is checked next. If defined and the source is not listed, it's a violation.
3. If neither rule applies, the import is allowed.

**Type-only imports.** `import type { Foo } from './bar'` can be allowed even when runtime imports from that module are forbidden. Use `allow_type_imports_from` to permit type sharing across boundaries without runtime coupling.

**Public entrypoint enforcement.** When a module declares `public_api: ["index.ts"]`, any external module importing from files other than the declared entrypoints is a violation. `import from 'api/orders/index'` → allowed. `import from 'api/orders/internal/helper'` → violation.

### Built-in constraint rules (MVP)

| Rule ID | Description | Params |
|---------|-------------|--------|
| `no-circular-deps` | No circular dependencies within or between specified modules | `scope: "internal" \| "external" \| "both"` |
| `enforce-layer` | Layer architecture enforcement — lower layers cannot import from higher | `layers: string[]` (ordered top→bottom) |

That's it for MVP. Two constraint rules plus boundary enforcement and dependency boundaries. Specgate is not a linter. It verifies architectural intent.

### Example 1: API service module

```yaml
version: "2"
module: api/orders
description: "Order management API service"

boundaries:
  path: "src/api/orders/**/*"
  public_api: ["index.ts"]
  allow_imports_from:
    - shared/types
    - shared/utils
    - services/orders
    - services/payments
    - lib/validation
  never_imports:
    - ui/**
    - database/**
    - admin/**
  allow_type_imports_from:
    - database/models       # Type sharing OK, runtime coupling forbidden
  allowed_dependencies:
    - zod
    - express
  forbidden_dependencies:
    - pg
    - prisma               # Must go through services layer
    - child_process

constraints:
  - rule: no-circular-deps
    params:
      scope: both
    severity: error
```

### Example 2: React component module (browser code)

```yaml
version: "2"
module: ui/checkout
description: "Checkout flow UI components"

boundaries:
  path: "src/ui/checkout/**/*"
  public_api: ["index.ts", "types.ts"]
  allow_imports_from:
    - ui/shared
    - ui/cart
    - hooks/**
    - types/**
    - lib/formatting
  never_imports:
    - api/**               # Components don't call APIs directly — use hooks
    - database/**
    - services/**
  allow_type_imports_from:
    - api/orders            # Can use OrderResponse type, not runtime imports
  allowed_dependencies:
    - react
    - react-dom
    - "@tanstack/react-query"
    - clsx
  forbidden_dependencies:
    - fs
    - path
    - child_process
    - net
    - pg
    - prisma

constraints:
  - rule: no-circular-deps
    params:
      scope: both
    severity: error
```

### Example 3: Shared library with layer enforcement

```yaml
version: "2"
module: core
description: "Core business logic — no framework dependencies"

boundaries:
  path: "src/core/**/*"
  public_api: ["index.ts"]
  allow_imports_from:
    - shared/types
    - lib/utils
  never_imports:
    - ui/**
    - api/**
    - cli/**
  allowed_dependencies:
    - zod
    - date-fns
  forbidden_dependencies:
    - react
    - express
    - next
    - "@tanstack/react-query"
    - fs
    - child_process

constraints:
  - rule: enforce-layer
    params:
      layers: [api, core, lib, shared]
    severity: error

  - rule: no-circular-deps
    params:
      scope: both
    severity: error
```

### Test file exclusion

Test files (`*.test.ts`, `*.spec.ts`, `*/__tests__/*`) are excluded from boundary enforcement by default. Test files legitimately import internals for unit testing — forcing them through public entrypoints would make testing impossible or require exporting everything.

Default exclusion patterns (configurable in `specgate.config.yml`):
```yaml
test_patterns:
  - "**/*.test.ts"
  - "**/*.test.tsx"
  - "**/*.spec.ts"
  - "**/*.spec.tsx"
  - "**/__tests__/**"
  - "**/__mocks__/**"
```

Test files are still checked against `forbidden_dependencies` (a test importing `fs` in a browser module is still a signal) but are exempt from `allow_imports_from`, `never_imports`, and `public_api` rules.

Override per-module if needed:
```yaml
boundaries:
  enforce_in_tests: true  # Opt-in: apply boundary rules to test files too
```

### Severity levels

- **error**: Hard fail. CI blocks. Agent must fix before proceeding.
- **warning**: Soft fail. CI reports but doesn't block. Flags for human review.

Warnings enable gradual adoption. Start with everything as `warning`, review violations, promote to `error` as specs stabilize.

---

## 6. Engine Architecture (MVP)

### Pipeline

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Spec Files  │────▶│  Spec Parser │────▶│ Validated     │
│  (.spec.yml) │     │  + Validator │     │ Spec Objects  │
└──────────────┘     └──────────────┘     └──────┬───────┘
                                                  │
┌──────────────┐     ┌──────────────┐             │
│  tsconfig +  │────▶│  Resolution  │─────────────┤
│  pkg.json    │     │  Map Builder │             │
└──────────────┘     └──────────────┘             │
                                                  │
┌──────────────┐     ┌──────────────┐     ┌──────▼───────┐
│  Codebase    │────▶│  AST Parser  │────▶│    Rule       │
│  (source)    │     │  (syntactic  │     │  Evaluator    │
└──────────────┘     │   only)      │     └──────┬───────┘
                     └──────────────┘            │
┌──────────────┐                          ┌──────▼───────┐
│  Git Diff    │─────────────────────────▶│  Verdict     │
│  (optional)  │                          │  Builder     │
└──────────────┘                          └──────┬───────┘
                                                 │
                                          ┌──────▼───────┐
                                          │  JSON Output │
                                          │  + Summary   │
                                          └──────────────┘
```

### Components

**Spec Parser.** Reads `.spec.yml` files. Validates against JSON Schema. Returns typed spec objects. Discovery: searches for `*.spec.yml` in project root and configured directories. Also supports explicit spec paths in `specgate.config.yml`.

**Resolution Map Builder.** Reads `tsconfig.json` (including `extends`), `package.json` workspaces, and pnpm/yarn workspace configs. Builds a complete alias-to-path resolution map. This component is the most critical piece of the engine — see Section 4.

**AST Parser.** Parses source files into ASTs using **syntactic traversal only**. Explicitly: we do NOT use ts-morph's type checker, which invokes the full TypeScript compiler and is notoriously slow. We use ts-morph (or a lighter alternative like @swc/core) for syntactic AST parsing only — extracting import/export declarations, re-export chains, and file-level structure. Build a dependency graph: which files belong to which module, what each file imports (with resolved paths) and exports. Discard ASTs after extracting the dependency graph to minimize memory.

**Rule Evaluator.** Takes validated specs + dependency graph + resolution map. Evaluates each rule:
- Boundary rules: check every import against `allow_imports_from`, `never_imports`, and `public_api` constraints.
- Dependency rules: check third-party imports against `allowed_dependencies` and `forbidden_dependencies`.
- Type-only distinction: classify `import type` statements separately and check against `allow_type_imports_from`.
- Constraint rules: dispatch to built-in rule implementations (`no-circular-deps`, `enforce-layer`).
- Produces a list of violations with file, line, rule, and message.

**Verdict Builder.** Aggregates violations across all specs. Filters escape hatches (`@specgate-ignore`). Applies severity. Produces structured JSON output and human-readable summary. Exit codes: 0 = pass, 1 = errors found, 2 = config/parse error.

**Determinism guarantee.** Same commit + same config = byte-identical JSON output. Violations are sorted by file path, then line number, then rule ID. No timestamps in the violations array (only in the top-level metadata). This enables diffing verdicts across runs and using verdicts as cache keys.

### Diff-aware mode with global dependency graph

When invoked with `--diff HEAD~1`, the engine cannot just check changed files. A change to module A may violate the boundary specs of modules that import from A. The engine must:

1. Build (or load from cache) a **global dependency graph** of the entire project.
2. Parse the diff to identify changed files.
3. Determine which modules contain changed files.
4. Determine which modules **import from** changed modules (the blast radius).
5. Run specs for all affected modules — both changed and dependents.
6. Cache the global dependency graph with file-hash invalidation for subsequent runs.

This is the only way diff-aware mode can be correct. Checking only changed files will miss transitive violations.

### Output format

```json
{
  "specgate": "2.0",
  "verdict": "FAIL",
  "timestamp": "2026-02-25T10:30:00Z",
  "duration_ms": 1240,
  "specs_checked": 12,
  "rules_evaluated": 47,
  "passed": 44,
  "failed": 3,
  "warnings": 1,
  "suppressions": {
    "total": 7,
    "new_in_diff": 1
  },
  "violations": [
    {
      "spec": "src/api/orders/orders.spec.yml",
      "module": "api/orders",
      "rule": "boundaries.never_imports",
      "severity": "error",
      "message": "Module 'api/orders' imports from forbidden module 'database'",
      "file": "src/api/orders/utils.ts",
      "line": 14,
      "column": 1,
      "import_source": "../../database/connection",
      "resolved_to": "src/database/connection.ts",
      "fix_hint": "Import from 'services/orders' instead of accessing the database directly"
    },
    {
      "spec": "src/ui/checkout/checkout.spec.yml",
      "module": "ui/checkout",
      "rule": "boundaries.public_api",
      "severity": "error",
      "message": "External import bypasses public API — must import through 'index.ts'",
      "file": "src/ui/cart/CartTotal.tsx",
      "line": 3,
      "column": 1,
      "import_source": "../checkout/internal/pricing",
      "resolved_to": "src/ui/checkout/internal/pricing.ts",
      "fix_hint": "Import from 'ui/checkout' (index.ts) instead of internal files"
    },
    {
      "spec": "src/ui/checkout/checkout.spec.yml",
      "module": "ui/checkout",
      "rule": "boundaries.forbidden_dependencies",
      "severity": "error",
      "message": "Module 'ui/checkout' imports forbidden dependency 'fs'",
      "file": "src/ui/checkout/export.ts",
      "line": 1,
      "column": 1,
      "import_source": "fs",
      "fix_hint": "Browser modules cannot use Node.js built-ins. Use a browser-compatible alternative."
    }
  ],
  "summary": "3 errors, 1 warning across 12 specs. 7 suppressions (1 new)."
}
```

### Performance targets

| Scenario | Target |
|----------|--------|
| Typical project (50 files, 5 specs) | < 2 seconds |
| Medium project (500 files, 30 specs) | < 5 seconds |
| Large monorepo (5000+ files, 100+ specs) | < 30 seconds |
| Diff-aware, single file change | < 1 second |

**Performance strategy:** Syntactic AST parsing only — never invoke the TypeScript type checker. Extract the dependency graph and discard ASTs immediately. Cache the global dependency graph with file-hash invalidation. Parallelize spec evaluation across modules. Parse lazily: only files referenced by specs.

### Language support

**MVP: TypeScript only.** Largest agent-coding market. Best AST tooling. Most complex module resolution (which is why we solve it first).

**Post-MVP:** Python (via tree-sitter or ast module), then Go (via go/ast). The rule evaluator is language-agnostic — it operates on a normalized dependency graph. Adding a language means adding a parser that produces the same graph format.

---

## 7. Key Design Decisions

**TypeScript first.** Largest market for agent-generated code. Most complex module resolution system, meaning solving it first proves the hardest case.

**Syntactic analysis only.** No type checker, no semantic analysis, no ts-morph Program. Syntactic AST traversal extracts imports, exports, and re-export chains. This keeps the engine fast and avoids the single biggest performance trap in TypeScript tooling.

**Public entrypoints, not symbol lists.** Modules declare which files are their public API (`public_api: ["index.ts"]`), not which symbols they export. This is structurally trivial to check, impossible to get out of sync, and eliminates merge conflicts from export lists.

**CLI first, CI second, IDE third.** The CLI is the foundation. CI integration is a thin wrapper. IDE integration comes after the core is proven.

**Machine-readable output with human-readable summary.** JSON primary (agents parse it, CI consumes it). Human summary to stderr. Both always produced.

**Gradual adoption.** Zero specs = zero overhead. One spec = value for that one module. No "rewrite your architecture" prerequisite.

**No AI in verification.** The verification loop is deterministic: spec + code → verdict. No LLM, no heuristics, no confidence scores. Binary pass/fail. This is the core differentiator.

**YAML, not a DSL.** Universally known, mature tooling, zero adoption barrier. Sufficient for Phase 1.

**Boundary-focused, not lint-focused.** Specgate verifies architectural intent: module boundaries, layer enforcement, dependency control. It does not enforce code style, file length, export patterns, or AST-level code patterns. Those belong in ESLint/Semgrep.

**Proactive AND reactive.** Specgate specs are YAML — directly consumable by any LLM. Feed `.spec.yml` files to coding agents BEFORE they start writing code and the agent knows the boundaries upfront. Then verify after. This is upstream prevention + downstream detection. No special integration needed — include the spec file in the agent's context window alongside the task description. This flips the value prop from "catch bugs" to "prevent bugs + catch the ones that slip through."

---

## 8. Escape Hatch Governance

### `@specgate-ignore` comments

Suppress a violation on a specific line:

```typescript
// @specgate-ignore: legacy auth adapter requires direct admin import during migration
import { verifyAdmin } from '../../admin/permissions';
```

### Rules

1. **Reason is required.** A bare `@specgate-ignore` without a reason is itself a violation. No exceptions.

2. **Optional expiry dates.** Encourage time-boxing suppressions:
   ```typescript
   // @specgate-ignore until:2026-04-01: migrating auth module to new boundary
   import { verifyAdmin } from '../../admin/permissions';
   ```
   After the expiry date, the suppression stops working and the violation surfaces again.

3. **CI reporting.** Every verdict includes:
   - Total active suppressions across the project
   - New suppressions introduced in the current diff
   - Expired suppressions that need resolution

4. **Configurable max new ignores per PR.** In `specgate.config.yml`:
   ```yaml
   escape_hatches:
     max_new_per_diff: 3    # CI fails if more than 3 new @specgate-ignore in a single PR
     require_expiry: false   # Set to true to require expiry dates on all ignores
   ```
   This prevents mass-suppression as a workaround for fixing real violations.

---

## 9. What Specgate MVP Will NOT Catch

Honesty about scope prevents a false sense of security.

**Specgate MVP catches:** Boundary violations, forbidden imports, architectural layer violations, circular dependencies, unauthorized third-party dependency usage, public API bypass.

**Specgate MVP does NOT catch:**

- **Semantic drift.** Code that satisfies all boundary constraints but implements the wrong business logic. (Phase 3 target.)
- **Logic errors.** Off-by-one, wrong comparison operator, incorrect algorithm. These require behavioral verification or formal proofs.
- **Performance regressions.** A function that runs 100x slower but produces correct output.
- **UX bugs.** Wrong color, broken layout, bad accessibility.
- **Subtle data bugs.** Data corruption that respects module boundaries. A function that rounds incorrectly but is called from the right module.
- **Runtime failures.** Null pointer exceptions, unhandled promise rejections, race conditions.
- **Security vulnerabilities beyond dependency control.** SQL injection, XSS, auth bypass within a correctly-bounded module.

Specgate verifies that code respects declared architectural intent. It does not verify that code is correct within those boundaries. That requires Phase 2 (invariants) and Phase 3 (behavioral verification).

---

## 10. Execution Roadmap

### Weeks 1–2: Foundation + Module Resolution

This is the hardest and most important phase. Module resolution correctness determines whether the entire engine is trustworthy.

- Initialize repository. TypeScript monorepo (Turborepo or Nx). MIT license.
- Toolchain: TypeScript 5.x, Vitest, ESBuild.
- **Build the resolution map builder first.** Read `tsconfig.json` (including `extends` chains), extract `paths`, `baseUrl`, and `rootDirs`. Read `package.json` workspace configs. Build complete alias → real-path resolution map.
- Build AST parser: syntactic-only traversal. Extract import declarations, export declarations, re-export chains (`export * from`). Resolve all import specifiers to real file paths using the resolution map. Output: normalized dependency graph.
- Handle edge cases from day one: `index.ts` fallbacks, `.ts`/`.tsx`/`.js` extension resolution, workspace symlink traversal, barrel file re-export chains.
- Define YAML spec schema (v2) as JSON Schema. Publish for editor autocomplete.
- Implement spec file discovery and parser/validator.
- **Start the golden corpus test suite.** Create fixture projects with known violations covering real agent bugs. Every known failure mode becomes a regression test. Track: defect type, does the engine catch it, if not why.
- Minimum 90% coverage on foundation code.

### Weeks 3–4: Core Boundary Enforcement

- Implement boundary enforcement:
  - `allow_imports_from`: resolve import paths, check against allowlist. When defined, module is in default-deny mode.
  - `never_imports`: check against denylist. Overrides everything. Any match = violation.
  - `public_api`: verify external imports only use declared entrypoint files.
  - `allow_type_imports_from`: classify `import type` statements and apply separate rules.
- Implement dependency boundaries:
  - `allowed_dependencies`: when defined, only listed third-party packages are permitted.
  - `forbidden_dependencies`: hard deny for specific packages.
  - Distinguish first-party (resolved to project files) from third-party (resolved to `node_modules`).
- Implement `no-circular-deps`: detect cycles in the dependency graph using Tarjan's or similar.
- Implement `enforce-layer`: given ordered layers, verify no upward imports.
- Build verdict builder: aggregate violations, apply severity, produce JSON + human summary.
- Implement diff-aware mode: build global dependency graph, parse git diff, identify blast radius (changed modules + their importers), scope verification.
- Expand golden corpus with fixtures for every rule type.

### Weeks 5–6: CLI + CI + Escape Hatches

- Build CLI:
  - `specgate check` — run verification, output verdict
  - `specgate check --diff HEAD~1` — diff-aware mode
  - `specgate init` — create `specgate.config.yml` and example spec
  - `specgate validate` — validate spec files without running checks
  - `specgate doctor [path]` — diagnostic command. Two modes:
    - `specgate doctor` (no args): show all module mappings, alias resolution table, spec file discovery results, and any config warnings.
    - `specgate doctor src/api/orders/utils.ts` (file path): show how the engine sees this specific file — which module it belongs to, which specs apply, how each of its imports resolves (with full resolution chain for aliased/re-exported paths), and which boundary rules would apply. File-level diagnostics are critical for debugging false positives.
- Implement `@specgate-ignore` parsing:
  - Require reason (bare ignore = violation)
  - Parse optional `until:YYYY-MM-DD` expiry dates
  - Track and report suppressions in verdict output
  - Enforce `max_new_per_diff` from config
- GitHub Actions integration: publish action to marketplace.
- Generic CI script: `npx specgate check --ci` with appropriate exit codes.
- Error messages with fix hints for every violation type.
- Documentation: README, spec language reference, getting started guide, CI setup.

### Weeks 7–8: Dogfooding + Hardening + Ship

- Write Specgate specs for real internal projects (OpenClaw, Hearth, etc.).
- Run Specgate in CI on those projects. Iterate on spec language based on real friction.
- **Validate the golden corpus.** Catalog 10+ real agent-produced bugs. Write specs that would have caught them. Verify the engine catches them. Document any gaps.
- Performance profiling and optimization. Verify targets: <2s typical, <30s monorepo.
- Edge case hardening: deeply nested re-exports, circular re-exports, dynamic imports (flag as unresolvable with warning), conditional exports.
- Dependency graph caching: serialize to disk, invalidate by file hash, verify cache correctness.
- Ship v0.1.0 to npm.

### Beyond MVP

- **`specgate infer` — AI-assisted spec generation.** AI scans existing codebase, analyzes module graph, proposes specs based on implicit boundaries. "This module only imports from these three modules — should that be a rule?" Human reviews and approves. Collapses cold-start problem.
- **Phase 2: Runtime invariants.** Typed expression language for invariants. Explicit bindings layer. Auto-injected assertions.
- **Phase 3: Behavioral verification.** Typed events. Behavioral baseline recording. Snapshot diffing.
- **Python support.** Parser via tree-sitter. Same rule evaluator, different AST frontend.
- **Go support.** Parser via go/ast.
- **IDE integration.** VS Code extension: inline violations, spec authoring with autocomplete.
- **`no-pattern` rule.** AST selector matching for forbidden code patterns. Deferred from MVP because it's tightly coupled to TypeScript AST and breaks when adding other languages. Leave pattern matching to ESLint/Semgrep for now.
- **Agent middleware.** Library for coding agents to receive specs as structured context before starting a task.

---

## 11. Open Questions (Acknowledged)

These are known unknowns. They don't block the MVP but need resolution before or during Phase 2.

**Expression language for invariants (Phase 2).** Three candidates: CEL (Common Expression Language), TypeScript expression subsets, or a custom DSL. Leaning CEL. Decision deferred to Phase 2.

**Cross-module spec composition.** When module A declares `public_api: ["index.ts"]` and module B declares `allow_imports_from: [api/orders]`, the engine verifies B only imports through A's public entrypoints. Explicit contract composition semantics are a Phase 2 concern.

**Monorepo support depth.** MVP: single `specgate.config.yml` with workspace-aware resolution. Multiple config files per workspace package may be needed. Revisit during dogfooding.

**Spec conflict resolution.** Two teams write conflicting specs. MVP: specs are authoritative per module — the module owner's spec wins. Cross-module conflicts surface as verification failures in the importing module.

**Generated code.** GraphQL codegen, Prisma client, protobuf stubs. MVP: exclude via glob patterns (`exclude: ["**/generated/**"]`). Long-term: first-class generated code support.

**Dynamic imports.** `import()` expressions with variable arguments are unresolvable statically. MVP: flag as a warning, do not attempt to resolve. Document the limitation.

**SWC vs ts-morph for parsing.** ts-morph provides the most complete TypeScript AST but carries weight. @swc/core is faster but may miss edge cases in re-export resolution. **Decision: start with ts-morph for correctness. Benchmark at end of week 2. Only switch to SWC if performance targets are missed on medium-size projects (500 files).** Correctness is non-negotiable; speed is optimizable. This decision affects the entire parser architecture and must be settled before week 3.
