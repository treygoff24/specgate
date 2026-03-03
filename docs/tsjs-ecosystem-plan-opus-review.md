# TS/JS Ecosystem Plan — Architecture Review

**Reviewer:** Opus subagent  
**Date:** 2026-03-03  
**Scope:** Full review of `tsjs-ecosystem-plan.md` against current Specgate code, OpenClaw's actual structure, and TS/JS ecosystem patterns  

---

## Executive Summary

The plan is well-grounded — the current-state analysis (Section 0) is accurate and evidence-backed. The phased structure is sound and the risk identification is honest. However, the review found **one critical performance issue** that would make Specgate unusable on OpenClaw-scale repos before any of the planned work even matters, **several important gaps** in TS/JS pattern coverage, and a handful of improvements to acceptance criteria and wave structure.

---

## Section 0: Current-State Summary

### **Critical: WalkDir traverses node_modules during module map construction**

**What's wrong:** `build_module_map_with_diagnostics` (mod.rs:168-178) calls `WalkDir::new(project_root)` with zero filtering. It walks **every file** under the project root, including `node_modules/`. On OpenClaw, this means walking hundreds of thousands of files across ~200+ npm packages. The glob matching against compiled specs will reject them all, but the I/O cost of walking the entire `node_modules` tree is catastrophic — easily 10-30 seconds on a cold filesystem.

**Why it matters:** This isn't a future concern — it's a blocker for using Specgate on any real JS/TS project today. The plan never mentions it, which means no task addresses it.

**What to do:** Add a new task in P1 (suggest P1.0 or fold into P1.1) to add directory filtering to the WalkDir traversal. At minimum, skip `node_modules/` directories. Ideally, respect `.gitignore` or a configurable exclude list. This is prerequisite to any meaningful testing on OpenClaw.

**Rating: Critical**

### Important: Resolver `condition_names` missing `"types"` condition

**What's wrong:** The plan's current-state analysis correctly notes `condition_names` at mod.rs:331-335 but doesn't flag that the list is `["import", "require", "default"]`. OpenClaw's `package.json` `exports` field uses `"types"` as a condition key:

```json
"./plugin-sdk": {
  "types": "./dist/plugin-sdk/index.d.ts",
  "default": "./dist/plugin-sdk/index.js"
}
```

Without the `"types"` condition, oxc_resolver will resolve `openclaw/plugin-sdk` to the `.js` file (via `"default"`), which is correct at runtime but diverges from how TypeScript resolves it during type-checking. For Specgate's purposes (architecture enforcement, not execution), the `"default"` path is likely fine, but this is worth documenting explicitly.

**Why it matters:** If `doctor compare` is meant to match TypeScript's resolution behavior, and TypeScript's resolver selects the `"types"` condition path, there will be systematic divergences that confuse users.

**What to do:** Add a note in P2.1 (alias parity hardening) to evaluate whether the `"types"` condition should be included and under what circumstances. Document the decision either way.

**Rating: Important**

---

## Section 1: Acceptance Criteria

### Important: Criterion 2 is missing several real-world import patterns found in OpenClaw

**What's wrong:** The acceptance criteria list these patterns:
- Relative imports with extension swapping
- Package `exports` conditions
- `tsconfig` path aliases
- Barrel re-export traversal
- First-party vs third-party classification

But OpenClaw also uses these patterns, which are absent:

1. **`import type` / `export type` (type-only imports):** Heavily used throughout OpenClaw (e.g., `import type { OpenClawConfig } from "../config/types.js"`). Specgate's parser needs to recognize these to avoid counting type-only imports as dependency edges in architecture enforcement (or at least provide a policy option). A module that only imports types from another module is architecturally different from one that has a runtime dependency.

2. **`import ... with { type: "json" }` (import attributes/assertions):** OpenClaw uses these for JSON imports (e.g., `import HOST_ENV_SECURITY_POLICY_JSON from "./host-env-security-policy.json" with { type: "json" }`). The parser needs to handle this syntax without choking. The `with` clause is part of the import statement grammar.

3. **Dynamic `import()` expressions:** OpenClaw test files use dynamic imports heavily for module isolation (`await import("./control-ui-assets.js")`). These create runtime dependency edges that static analysis can't always resolve. The plan should state whether dynamic imports are in-scope or explicitly out-of-scope.

4. **`export * from` (wildcard re-exports):** Used in OpenClaw's infra layer (e.g., `export * from "./exec-approvals-analysis.js"`). These create transitive dependency chains that affect barrel file analysis.

**Why it matters:** If the acceptance bar doesn't include these patterns and they cause parser errors or misclassification on the target project, the "works on real JS/TS projects" claim falls apart immediately.

**What to do:** Add sub-bullets under criterion 2 for: type-only imports (policy decision: count or ignore?), import attributes (`with { type: ... }`), dynamic imports (in-scope or not?), wildcard re-exports. Even if the answer is "handled by oxc parser already," state it explicitly.

**Rating: Important**

### Suggestion: Criterion 3 ("useful violations without brittle manual config surgery") is too vague to test

**What's wrong:** "Useful violations" and "brittle manual config surgery" aren't measurable. What counts as useful? How much config is too much?

**What to do:** Make it concrete: "On OpenClaw, `specgate init && specgate check` produces violations that correctly identify at least one real cross-module dependency (e.g., an extension importing from core), with no more than N manual config edits to the generated spec files." Pick a number for N.

**Rating: Suggestion**

---

## Section 2: Phased Implementation

### P1.1 (Workspace Discovery)

**Suggestion: Should also read `package.json` `workspaces` field (not just pnpm-workspace.yaml)**

**What's wrong:** The task description says "reads root `package.json`, `pnpm-workspace.yaml`." But npm and Yarn define workspaces in `package.json` under the `"workspaces"` field, not in a separate YAML file. OpenClaw uses pnpm, but for the feature to be generically useful, both should be supported.

**What to do:** Add acceptance criterion: "Discovery also supports `workspaces` field in `package.json` (npm/Yarn convention)."

**Rating: Suggestion**

### P1.3 (Nearest tsconfig)

**Important: OpenClaw extensions DON'T have their own tsconfigs**

**What's wrong:** The task assumes nested tsconfigs per workspace package. In OpenClaw's actual structure, there are zero tsconfigs under `extensions/` or `packages/`. The root `tsconfig.json` has `"include": ["src/**/*", "ui/**/*", "extensions/**/*"]` — it covers everything in one config. The only nested tsconfigs are under `vendor/a2ui/` (third-party vendored code) and one `tsconfig.plugin-sdk.dts.json` at root (for declaration generation only).

This means the "nearest tsconfig" strategy would find `vendor/a2ui/renderers/lit/tsconfig.json` for files under `vendor/`, which may be correct, but for all actual first-party code, the root tsconfig is the only one.

**Why it matters:** The task is rated "High" complexity. If the primary target project doesn't even exercise the feature, the complexity investment may be premature. The root-only behavior (already implemented) covers OpenClaw.

**What to do:** Consider downgrading P1.3 to P2 or making it conditional. The current root-only behavior may be sufficient for MVP. Add a note: "Root-only tsconfig covers OpenClaw; nearest-tsconfig is needed for repos with per-package tsconfig (e.g., Turborepo-style). Defer if not blocking dogfood."

**Rating: Important**

### P1.4 (First-party Classification)

**Important: Dual resolution path for workspace packages needs explicit handling**

**What's wrong:** In OpenClaw, `openclaw/plugin-sdk` resolves through TWO mechanisms simultaneously:
1. **tsconfig `paths`:** `"openclaw/plugin-sdk": ["./src/plugin-sdk/index.ts"]` — maps to source file
2. **package.json `exports`:** `"./plugin-sdk": { "types": "...", "default": "./dist/plugin-sdk/index.js" }` — maps to built output

With `moduleResolution: "NodeNext"`, TypeScript uses the `exports` field for bare specifier resolution, and `paths` as an override/alias. oxc_resolver with `TsconfigReferences::Auto` should follow the same precedence, but this is a known source of mismatches.

The classification issue: if oxc_resolver resolves `openclaw/plugin-sdk` to `./dist/plugin-sdk/index.js` (via exports), the resolved path is inside the project root but points to build artifacts, not source. If it resolves to `./src/plugin-sdk/index.ts` (via paths), it's correct source.

**Why it matters:** Classifying correctly depends on *which* resolution path wins, and this varies by resolver configuration. The plan's acceptance criterion ("can be classified as first-party when it resolves to workspace code") needs to specify: does "workspace code" mean source or built output?

**What to do:** Add acceptance criterion: "Resolution of workspace specifiers through tsconfig paths vs package.json exports produces consistent classification. Built output (`dist/`) is either excluded from module map or handled equivalently to source."

**Rating: Important**

### P2.3 (Barrel/Re-export Robustness)

**Suggestion: Should explicitly address `export *` vs `export { named }` distinction**

**What's wrong:** OpenClaw uses both `export * from` (transitive, all symbols) and `export { specific } from` (selective re-export). The plan mentions "re-export-heavy packages" but doesn't distinguish between these two patterns, which have very different implications for dependency graph construction. `export *` creates an implicit dependency on the entire target module's public surface; `export { named }` is scoped.

**What to do:** Add sub-criteria distinguishing wildcard vs named re-exports and how each affects edge construction.

**Rating: Suggestion**

---

## Section 4: Wave Structure

### Important: A2 and A3 should NOT be parallel — they share too much surface

**What's wrong:** The plan correctly flags A2/A3 as conflicting ("both touch resolver internals") but still groups them in the same wave with a vague note to "isolate shared edit windows." In practice, A2 (tsconfig strategy refactor in `mod.rs`) touches the `build_resolve_options` function and potentially `ModuleResolver::new`, while A3 (classification model update) touches `classify.rs` AND resolver plumbing in `mod.rs` (the `resolve_uncached` function that calls `classify_resolution`).

These aren't just "same file" conflicts — they're "same function" conflicts. `resolve_uncached` calls `classify_resolution` and uses the resolver options built by `build_resolve_options`. Changing both simultaneously would require constant rebasing.

**Why it matters:** In an autonomous multi-agent build, merge conflicts in the same function produce garbage. The "coordinate via isolated edit windows" mitigation is insufficient for agents that can't coordinate in real-time.

**What to do:** Sequence A2 → A3 (tsconfig resolution must be settled before classification can be updated to use workspace-aware context). Alternatively, keep A1 + A2 in Wave A, then A3 in a half-wave A' that depends on A2's merge.

**Rating: Important**

### Suggestion: B1/B2 conflict in `src/cli/mod.rs` is more severe than noted

**What's wrong:** The plan notes B1/B2 "may touch `src/cli/mod.rs`" — but `src/cli/mod.rs` is a >2800-line file that contains both `infer_init_scaffold_specs` (B1's target) and `doctor compare` logic (B2's target). While these are different functions, they're in the same monolithic file. Agents editing different parts of a 2800-line file still risk diff conflicts from context overlap.

**What to do:** Consider whether P1 should include a preliminary refactor to extract `init.rs` and `doctor.rs` from the monolith before Wave B begins. The plan already hints at this ("src/cli/init.rs (if split ownership)") — make it explicit.

**Rating: Suggestion**

---

## Section 5: Risks and Open Questions

### Important: Missing risk — Performance at scale (WalkDir + glob matching)

**What's wrong:** As noted in the Critical finding above, the module map builder walks every file under project root. Beyond the node_modules issue, OpenClaw has `vendor/`, `dist/`, `.git/`, `apps/android/` (Gradle project), `apps/ios/` (Xcode project), `apps/macos/`. Walking all of these is wasteful. The plan's risk section doesn't mention performance at all.

**What to do:** Add a risk item: "Module map construction performance on large repos. WalkDir currently traverses all files under project root including node_modules, dist, vendor, and platform-specific directories. Mitigation: implement configurable directory exclusion with sensible defaults (node_modules, dist, .git, build output)."

**Rating: Important**

### Suggestion: Open questions are missing one key question

**What's wrong:** The open questions are good, but missing: "Should type-only imports (`import type`) be treated as dependency edges for architecture enforcement purposes, or should they be excluded/optionally excluded?" This is a policy decision that affects the dependency graph significantly. In many architecture enforcement tools, type-only imports are either ignored or treated as a weaker dependency class.

**What to do:** Add to open questions: "Should `import type` create dependency edges? If yes, should they be distinguishable from runtime imports in policy rules?"

**Rating: Suggestion**

---

## Section 6: Execution Order

### Suggestion: Missing prerequisite step — node_modules exclusion

**What's wrong:** The execution order starts with P1.1 (workspace discovery), but as noted above, Specgate can't even run meaningfully on OpenClaw without first fixing the WalkDir traversal to skip node_modules. This should be step 0.

**What to do:** Insert step 0: "Fix module map builder to skip node_modules/ (and configurable excludes) during file discovery."

**Rating: Important (elevated from Suggestion because it blocks all testing)**

---

## Missing from Plan Entirely

### Important: Import attributes / `with { type: ... }` syntax support

**What's wrong:** OpenClaw uses `import ... with { type: "json" }` (the TC39 Import Attributes proposal, Stage 4). This is a syntactic extension to import statements. If Specgate's parser (likely oxc_parser) doesn't handle this, it will fail to parse these files entirely, breaking the dependency graph.

**Why it matters:** Parser failure on even one file can cascade — if a file can't be parsed, its imports are lost, and modules that depend on it may appear disconnected.

**What to do:** Verify that oxc_parser handles import attributes. If it does, add a test fixture. If it doesn't, this becomes a P1 blocker. Either way, document it.

**Rating: Important**

### Suggestion: `vendor/` directory handling

**What's wrong:** OpenClaw has a `vendor/a2ui/` directory with its own tsconfigs, source code, and build artifacts. This is vendored third-party code that lives inside the project root but should probably be treated as third-party (or at least excluded from module map). The plan doesn't mention vendor directories.

**What to do:** Add a note under P1.4 or risks: "Vendored source directories (e.g., `vendor/`) inside project root need classification rules — they're first-party by path but third-party by intent."

**Rating: Suggestion**

### Suggestion: `.d.ts` declaration files

**What's wrong:** OpenClaw generates `.d.ts` files (via `tsconfig.plugin-sdk.dts.json`). These live in `dist/` (which should be excluded from module map), but if someone configures a broader glob, `.d.ts` files would be discovered. They contain only type information and import statements, which could create phantom dependency edges.

**What to do:** Consider whether `.d.ts` files should be excluded from file discovery by default, or at least documented as a known edge case.

**Rating: Suggestion**

---

## Complexity Assessments

The complexity ratings are mostly realistic with one exception:

**P1.3 (nearest tsconfig) rated "High" — agree, but may be over-scoped for MVP.** As noted, OpenClaw doesn't exercise this. Consider splitting into "High complexity, Low priority for MVP" and deferring.

**P1.4 (first-party classification) rated "High" — agree.** The dual resolution path (tsconfig paths vs package.json exports) makes this genuinely complex.

**P2.3 (barrel robustness) rated "Medium" — may be under-rated.** OpenClaw's barrel patterns include `export *`, selective re-exports, and type re-exports. Getting all of these right in the dependency graph is fiddly. Consider "Medium-High."

---

## Summary of Findings by Priority

| Rating | Count | Key Items |
|--------|-------|-----------|
| Critical | 1 | WalkDir traverses node_modules — blocks all real-world usage |
| Important | 7 | Missing import patterns in acceptance criteria; extensions lack tsconfigs (P1.3 over-scoped); dual resolution path for workspace packages; A2/A3 shouldn't be parallel; missing performance risk; import attributes syntax; missing `types` condition |
| Suggestion | 6 | Vague acceptance criterion 3; npm/Yarn workspaces field; export * vs named re-exports; B1/B2 file split; type-only import policy question; vendor dir handling |

**Bottom line:** Fix the WalkDir node_modules traversal before anything else. Reconsider P1.3's priority (nearest tsconfig) since the target project doesn't use it. Tighten Wave A sequencing (A2 → A3, not parallel). Add import attributes and type-only imports to the acceptance criteria.
