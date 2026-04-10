I read both documents and they fit together cleanly: “Specgate” is a crisp, shippable Layer 1 that enforces structural intent deterministically via AST + module resolution, and the broader “Spec Engine” concept extends that into invariants and behavioral drift detection.

The core bet is correct: agentic coding fails in the “easy to generate, hard to verify” quadrant, and the only reliable way out is to move intent into something mechanically checkable, with a hard separation between implementation and verification.

Where I think you can make it meaningfully better is mostly about (1) narrowing MVP scope in the right places to avoid a correctness sinkhole, (2) filling a few missing product and governance primitives that matter specifically for agents, and (3) adding one or two rules that punch above their weight in real-world monorepos.

First: you need to explicitly position Specgate against the existing “architecture boundary” ecosystem and then lean hard into what you’re uniquely doing for agents.

There are already several ways to enforce JS/TS dependency boundaries: Nx’s `@nx/enforce-module-boundaries` rule, dependency-cruiser, JS Boundaries (eslint-plugin-boundaries), and good-fences. ([Nx][1]) If Specgate is framed as “enforce module boundaries,” people will bucket it with these. Your differentiator should be “agent-first deterministic verification substrate” with: (a) correct TS resolution across tsconfig/workspaces, (b) diff-aware blast radius as a first-class mode, (c) governance primitives for suppressions/spec changes, (d) machine-readable verdicts designed to drive an automated repair loop, and (e) a roadmap into invariants and behavioral drift. The other tools are good, but they’re primarily lint or architecture hygiene. For example, dependency-cruiser is explicitly “validate dependencies with your rules” and can generate reports/graphs; it’s not designed around agent feedback loops and governance. ([GitHub][2]) JS Boundaries is also “instant feedback in ESLint” and supports many dependency syntaxes; again, not an agent-verifier protocol. ([JS Boundaries][3])

If you want to align with the broader “spec-driven development” movement without being swallowed by it, explicitly say: GitHub Spec Kit is a process/tooling layer for spec-driven workflows, while Specgate is a deterministic verifier/gate that can be plugged into any workflow (including Spec Kit). ([The GitHub Blog][4])

Second: fix a contradiction that will matter immediately if you want “determinism” to be a trust anchor.

Specgate claims “same commit + same config = byte-identical JSON output,” but the example output includes `timestamp` and `duration_ms`. Those will never be byte-identical across runs. This is not pedantry. The moment you want to cache verdicts, diff them, or treat them as artifacts in an agent loop, non-deterministic metadata will cause flaky downstream behaviois to one of these patterns:

Option A (cleanest): make the JSON strictly deterministic by default. Remove timestamp/duration from the JSON, keep them in stderr or in a separate `--metrics-json` stream.

Option B: keep metadata but separate it into a `run_metadata` object and guarantee a `--deterministic-json` mode that omits it entirely, and document that only deterministic mode is safe for caching/verdict hashing.

Option C: include stable metadata only (git sha, config hash, spec hash, tool version), and drop wall-clock time entirely.

If you keep any non-deterministic fields, you should never promise byte-identical output.

Third: module resolution is rightly flagged as the hardest part. The biggest improvement is: do not reimplement TypeScript’s resolver. Use it.

You already call out tsconfig `paths`, `baseUrl`, workspace symlinks, index fallbacks, etc. But TypeScript’s own compiler API exposes the standard resolution via `resolveModuleName`, and it supports customization and caching. ([GitHub][5]) Reimplementing the algorithm is a multi-year footgun because TS/Node/bundleably package.json `exports`/`imports` and “bundler” resolution semantics). TypeScript explicitly documents multiple resolution strategies (`node16`, `nodenext`, `bundler`, etc.) and the semantics differ. ([TypeScript][6])

A practical architecture here is:

1. Parse tsconfig(s), determine which compiler options apply to each file (this matters in monorepos with project references).
2. Use `ts.resolveModuleName` with a `ModuleResolutionHost` and `ts.createModuleResolutionCache` (or equivalent caching) so resolution matches what TS would do, without creating a full Program/type-checker. ([GitHub][5])
3. Layer workspace package mapping on top only where TS can’t infer what you need (for example, providing a filesystem host that resolves symlinks consistently in a workspace).

This change alone collapses an enormous amount of edge-case risk. It also future-proofs you against TS adding new resolution behaviors.

Related: treat “resolution correctness” as a product surface. Your `specgate doctor` idea is excellent. I would add one more capability: “doctor compare” that shows Specgate resolution vs `tsc --traceResolution` (or the equivalent trace) for a given import, so users can prove parity when debugging.

Fourth: tighten MVP scope by cutting one thing that looks expensive but isn’t necessary for your stated MVP value.

The doc says “barrel file re-export chains must be followed to determine the true origin of symbols.” rules you list (boundary imports, public entrypoints, layer ordering, circular deps), you mostly do not need symbol-origin tracing. You need file-level dependency edges: “file A imports specifier X that resolves to file B (or package P).” Re-export symbol provenance becomes necessary when you want rules like “don’t re-export types from forbidden layers” or “public API must not expose internal modules transitivele, but they are not required to catch architectural erosion via direct imports.

I would make symbol-level export provenance a Phase 1.5 feature behind a flag, and keep MVP strictly file-edge-based. That reduces the number of “resolution” things you need to get right up front.

Fifth: add two missing concepts to the spec language that will materially improve adoption and reduce configuration churn.

1. Provider-side visibility / “who may import me”

Right now, the main control is importer-side (`allow_imports_from` puts the importer into default-deny). That works if every module has a spec and you enforce “no spec, no merge.” But most real repos adopt gradually, and you’ll always have “unspecified” areas.

You should add an optional provider-side constraint such as:

- `visibility: public | internal | private`
- `allow_imported_by: [module patterns]` (or `deny_imported_by`)
- `friend_modules: [...]` (for “only these can import this internal package”)

This is the same reanternal` visibility: it protects a module even when the consumer has no rules. good-fences captures a similar idea by defining “fences” around directories controlling what can pass in or out. ([GitHub][7])

2. Canonical import specifier for fix hints and for “no cross-module relative imports”

Your fix hints and public API enforcement examples assume you can tell people “import from ui/checkout” or “import from api/orders.” In real TS codebases, the canonical import path is often an alias (`@/ui/checkout`), a workspace package name (`@myorg/checkout`), or a Next.js/TS path mapping. Without an explicit canonical import ID, fix hints become wrong, and auto-fixes become impossible.

Add something like:

- `import_id: "@/ui/checkout"` or a list of acceptable IDs
- or `package: "@myorg/checkout"` for workspace libs

Then you can add a very high-value rule: “cross-module imports must uDs, not relative paths.” This solves a common monorepo hygiene problem and prevents “relative path sneaking” that bypasses boundary tooling. (This kind of concern shows up repeatedly in monorepo boundary discussions.) ([GitHub][8])

Sixth: clarify and harden “module ownership” semantics to avoid ambiguity and false positives.

Right now a spec says `module: api/orders` and a glob `boundaries.path: "src/api/orders/**/*"`. You need to define and enforce:

- No overlapping module boundary globs (or deterministic tie-break: “most specific path wins”).
- What happens to files not claimed by any module spec.
- Whether “module id” is purely an identifier, or whether it implies a path prefix for other rules like `enforce-layer`.

I would add an explicit “module registry” pass that fails fast on:

- overlaps,
- unclaimed files (optional, configurable),
- orphan specs (spec path matches no files),
- and “module id duplicates.”

This is wheron: users need confidence that a file belongs to exactly one module, and that a violation is not an artifact of spec ambiguity.

Seventh: broaden the dependency edge extractor slightly so you don’t miss common real-world dependency forms.

Specgate currently describes parsing import/export declarations. But JS Boundaries explicitly supports `require`, `exports`, and dynamic imports (`import()`), and even allows custom AST nodes like `jest.mock()` to count as dependencies. ([JS Boundaries][3]) Agents will absolutely introduce these patterns (sometimes because tools auto-generate them).

For MVP, I’d do:

- Support `require("literal")` as a dependency edge.
- Support `import("literal")` as a dependency edge.
- Treat non-literal dynamic imports as “unresolvable” with a warning (as you already propose). ther `jest.mock("module")` should count for boundary enforcement in tests (maybe warning-only).

This reduces the “agent got around the gate by using a different syntax” class of failures.

Eighth: strengthen the “diff-aware” story into something CI-usable without user footguns.

`--diff HEAD~1` is fine for local dev, but CI often needs “merge base with main,” and PRs can have multiple commits. Add:

- `--base <ref>` and `--head <ref>` (default to merge-base with origin/main when available).
- Emit in the JSON which refs werlast radius” set visible in the output (which modules were checked and why).

This matters for trust: if the gate fails on a module the developer didn’t touch, they need the explanation immediately.

Ninth: add a baseline mechanism distinct from `@specgate-ignore`.

The line-level ignore with reason/expiry is excellent governance. But for large existing repos, there’s a different adoptioner than sprinkling ignores: a “known violations baseline” file.

Pattern:

- `specgate baseline` generates a JSON file of current violations fingerprints.
- In CI, violations in the baseline are reported but don’t fail; new violations fail (or warn), and removing baseline items is encouraged.
- Baseline updates require an explicit command and can be locked down.

This avoids polluting code with ignore comments and makes “pay down architecture debt” measurable.

To make this work, yo “fingerprint” field per violation (hash of rule + importer file + resolved target + maybe module ids). That also makes it easier to de-duplicate violations across runs.

Tenth: recognize and explicitly address “spec-implementation collusion,” not just “test-implementation collusion.”

You already identify test collusion: same agent writes tests and implementation. But you can also get spec collusion: an agent changes the spec to match its code, and now verification “passes.” In an agent-heavy workflow, this will happen unless you design governance around it.

Add policy knobs such as:

- “spec files are protected”: CI fails if `.spec.yml` changes without a label or a CODEOWNERS approval.
- “spec-change budget”: changing specs in a PR requires extra review or higher bar.
- Output explicitly when spec files changed, and list which rules changed.

This aligns with the “constitution” idea popularized irules are meant to be non-negotiable. ([The GitHub Blog][4])

Eleventh: for Phase 2 and 3, the missing piece is not the checking mechanism, it’s the binding and baseline governance. Design those earlier than you think.

Runtime invariants

You call out an “explicit bindings layer mapping spec concepts to code constructs.” This is the make-or-break. If invariants are written as free-floating expressions without clear attachment points, you’ll either (a) not know where to inject them, or (b) inject everywhere and drown in noise/perf cost.

A workable approach is to require each invariant to declare at least one of:

- a function boundary (“assert after function X returns”),
- a type boundary (“assert whenever a value of type T is constructed/decoded”),
- or an event boundary (“assert whenever event E is emitted”).

You also need an expression language choice. CEL is reasonable. Another credible direction is using a policy language like OPA/Rego to express certain kinds of constraints ov facts, but it’s better suited to “policy over data” than arithmetic invariants. ([Open Policy Agent][9]) My suggestion: use CEL (or a small CEL-like subset) for invariants, and keep Rego as an optional future “policy plugin” if you want very flexible org-specific rules evaluated over facts extracted from code or runtime traces.

Behavioral snapshot diffing

The baseline problem is everything. The moment you have “record what the code does, then diff later,” you have reinvented snapshot testing and will fight flakes forever unless you lock down determinism. Your Phase 3 notes already gesture at this (trace normalization, deterministic replay). I’d make it explicit that:

- baseline updates are first-class and reviewable artifacts (like snapshots),
- baseline changes require human approval (or at least separate agent),
- traces must be canonicalized (time, randomness, ids) or you’ll get noise.

If you want to tie this into the broader research landscape, there’s recent work around “constitutional” spec constraints for secure-by-construction AI codegen, and separate lines of work around verifier models, but your deterministic approach stays cleanly distinct. ([arXiv][10])

Twelfth: one more MVP rule that is worth its weight in gold for agent workflows.

Add an “import hygiene” rule that bans deep imports into third-party packages (or at lExample: allow `lodash` but forbid `lodash/fp` (or vice versa), allow `@aws-sdk/client-s3` but forbid `@aws-sdk/*` generically, etc. Agents will commonly guess subpath imports that work in some bundlers but not others. Existing tools in this space emphasize dependency syntax breadth and configurable rules; you can do a narrow version that’s deterministic and high-signal. ([GitHub][2])

What I would remove or defer, concretely.

1. Defer symbol-origin tracing through re-export chains for MVP unless you have a specific rule that needs it. Keep file-level dependency edges first.

2. Consider deferring `allowed_dependencies` allowlists as a “recommended but optional” feature, because it can be config-heavy per module. Start with `forbidden_dependencies` and environment profiles (browser forbids Node builtins, etc.), then add allowlists when teams want strictness. This is more about adoption friction than technical difficulty.

3. If you need to cut further for speed to ship: keep `no-circular-deps` and boundary enforcement, and defer `enforce-layer` until you have a crisp layer-to-module mapping scheme (prefix-based, explicit `layer:` field, or tag mapping). Right now the mapping is implicit, and implicit mappings are where tools lose trust.

Net: what you have is already unusually strong because it is (a) concrete and MVP-shaped, (b) centered k, and (c) deterministic by design. The main upgrades I’d prioritize are: use TS’s resolver instead of reimplementing it, fix the determinism guarantee, add provider-side visibility and canonical import IDs, broaden dependency extraction to cover require/dynamic import literals, and add baseline/spec-governance so agents can’t “move the goalposts” without it being obvious.

[1]: https://nx.dev/docs/technologies/eslint/eslint-plugin/guides/enforce-module-boundaries "Enforce Module Boundaries ESLint Rule | Nx"
[2]: https://github.com/sverweij/dependency-cruiser "GitHub - sverweij/dependency-cruiser: Validate and visualize dependencies. Your rules. JavaScript, TypeScript, CoffeeScript. ES6, CommonJS, AMD."
[3]: https://www.jsboundaries.dev/docs/overview/ "Overview | JS Boundaries"
[4]: https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/ "Spec-driven development with AI: Get started with a new open source toolkit - The GitHub Blog"
[5]: https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API "Using the Compiler API · microsoft/TypeScript Wiki · GitHub"
[6]: https://www.typescriptlang.org/tsconfig/moduleResolution.html "TypeScript: TSConfig Option: moduleResolution"
[7]: https://github.com/smikula/good-fences "GitHub - smikula/good-fences: Code boundary management for TypeScript projects"
[8]: https://github.com/palantir/tslint/issues/3754?utm_source=chatgpt.com "Rule suggestion: disallow relative path imports outside the ..."
[9]: https://openpolicyagent.org/docs/policy-language?utm_source=chatgpt.com "Policy Language"
[10]: https://www.arxiv.org/abs/2602.02584?utm_source=chatgpt.com "Enforcing Security by Construction in AI-Assisted Code ..."
