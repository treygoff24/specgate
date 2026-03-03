# Specgate Resolver Review: TS/JS Ecosystem Plan

I have reviewed the `tsjs-ecosystem-plan.md` plan against the current Specgate `master` codebase and OpenClaw's real-world TS/JS monorepo patterns.

Here is the technical feasibility assessment focused on resolver correctness and edge cases.

## 1. Missing Extension Aliases for `NodeNext` (.js -> .ts)
**Rating:** Critical
**Motivation:** OpenClaw uses `"moduleResolution": "NodeNext"`. A standard pattern in NodeNext is importing `.ts` files using their `.js` output extension.
*OpenClaw evidence:* `import { buildPairingReply } from "./pairing-messages.js";` (inside `pairing-challenge.ts` which imports `pairing-messages.ts`).
**Finding:** The current `oxc_resolver::ResolveOptions` configured in `src/resolver/mod.rs:312-321` sets `extensions` but **completely omits `extension_alias`**. Without this, `oxc_resolver` will treat `"./pairing-messages.js"` as a literal file, fail to find it, and return `ResolvedImport::Unresolvable`.
**Recommendation:** Add `.js` -> `.ts`/`.tsx` (and `.jsx` -> `.tsx`) to `extension_alias` in the resolver options. This is a hard blocker for `NodeNext` TS projects.

## 2. P1.4 (Workspace Classification) Might Be Unnecessary / Overcomplicated
**Rating:** Important
**Motivation:** P1.4 proposes an "explicit first-party package-name map to resolve cases where package specifier points to a workspace package symlinked under `node_modules`."
*OpenClaw evidence:* `packages/clawdbot/package.json` depends on `"openclaw": "workspace:*"`.
**Finding:** The current resolver `build_resolve_options` already sets `symlinks: true`. This means `oxc_resolver` natively follows the pnpm `node_modules/openclaw` symlink to `/Users/treygoff/Development/openclaw/` *before* Specgate sees the path.
When `classify_resolution` evaluates the path, it receives the canonical, real path (e.g., `/repo/src/...`). Since this real path does *not* contain `node_modules/` and starts with `project_root`, it is already correctly classified as `ResolvedImport::FirstParty`.
**Recommendation:** Verify if `symlinks: true` already solves the workspace classification problem natively. You might only need P1.4 if you need to support non-symlinked monorepos (like pnpm `node-linker=hoisted` without workspace symlinks, which is rare) or if you want custom logical naming.

## 3. Missing `"node"` in Resolver `condition_names`
**Rating:** Important
**Motivation:** OpenClaw relies heavily on `package.json` `"exports"` fields.
*OpenClaw evidence:* `packages/clawdbot/package.json` has `"exports": { ".": "./index.js" }`.
**Finding:** The current `condition_names` are `["import", "require", "default"]`. However, for a Node-based TS codebase, many npm packages' `exports` fields rely on the `"node"` condition to route correctly (especially when distinguishing browser vs. node).
**Recommendation:** Add `"node"` to the `condition_names` array in `build_resolve_options` to mirror actual Node.js/TypeScript resolution behavior for these ecosystems.

## 4. P1.3 ("Nearest tsconfig") is Absolutely Necessary
**Rating:** Critical
**Motivation:** You flagged this as "High Complexity", but it's completely mandatory for OpenClaw's architecture.
*OpenClaw evidence:* OpenClaw defines path aliases in the root `tsconfig.json` (`"openclaw/plugin-sdk": ["./src/plugin-sdk/index.ts"]`), but there are nested tsconfigs in `vendor/a2ui/renderers/.../tsconfig.json`.
**Finding:** If a file inside `packages/clawdbot` imports `openclaw/plugin-sdk`, and the resolver only knows about `packages/clawdbot/tsconfig.json` (if it has one that doesn't define paths) OR the root one, it might miss local path overrides or project references. TypeScript's resolution context is per-file, based on the `tsconfig` that includes that file.
**Recommendation:** Proceed with P1.3. `oxc_resolver` has decent cache support, but instantiating a new resolver per tsconfig is the correct architectural move to avoid path alias leakage between workspace packages.

## 5. Barrel Files and Re-export Explosion (Task P2.3)
**Rating:** Suggestion
**Motivation:** The plan asks if barrel files / re-exports will cause an edge explosion.
*OpenClaw evidence:* Re-exports are used routinely (`export * from "./all";`).
**Finding:** I reviewed `src/parser/mod.rs`. Specgate parses `export * from "./all"` as a single `ReExportInfo` edge to `"./all"`. Because Specgate enforces boundaries at the *file edge* level and does not trace symbol origins transitively across barrel files, there will be **no edge explosion**. The graph remains exactly as complex as the file imports.
**Recommendation:** The complexity assessment for P2.3 (Medium) might actually be "Low" or a no-op, because the current parser architecture already limits barrel files to a single file-to-file edge. Just add fixtures to ensure policy evaluation treats re-exports as standard dependencies.

## 6. Dynamic Import Expressions (`import()`)
**Rating:** Suggestion
**Motivation:** OpenClaw uses dynamic imports.
*OpenClaw evidence:* `await import("./system-presence.js");` and `const { expandHomePrefix } = await import("./home-dir.js");`.
**Finding:** `src/parser/mod.rs` correctly extracts string literal dynamic imports, but flags template literals (e.g., ``await import(`./${name}`)``) as unresolved warnings. This is correct behavior for static analysis.
**Recommendation:** No architectural changes needed here. The parser is robust enough for OpenClaw's dynamic import patterns.

## Summary
The plan is highly accurate and feasible. The most critical missing piece is the `.js` -> `.ts` `extension_alias` missing in `oxc_resolver` config, which will immediately break on OpenClaw's `NodeNext` codebase unless fixed. Workspace symlinks (`symlinks: true`) will likely do the heavy lifting for classification, meaning P1.4 might be simpler than expected.