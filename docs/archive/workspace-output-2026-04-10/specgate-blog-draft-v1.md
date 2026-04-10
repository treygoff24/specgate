# The Tool Nobody Built: How We Made AI Agents Stop Breaking Their Own Architecture

AI agents can write code now. You've heard. Everyone's heard. Every week there's a new benchmark where some model scores 90% on SWE-bench or passes another batch of coding interviews, and the discourse cycles through the same loop: agents will replace developers, no they won't, actually they'll augment developers, actually nobody knows. The discourse is exhausting and mostly wrong, because it focuses on the wrong bottleneck.

The bottleneck isn't writing code. That's the easy part, and it's been the easy part for months. The bottleneck is knowing whether the code is correct.

Not "correct" in the trivial sense of "does it compile" or "do the tests pass." Correct in the structural sense: does this code respect the architecture? Does it import from the right modules? Does it maintain the boundaries that exist for a reason? When an agent generates 500 lines of perfectly functional TypeScript that silently reaches across a module boundary it shouldn't cross, the tests pass, the linter is happy, and you've introduced a dependency that will cost you weeks to untangle six months from now. The agent doesn't know it did anything wrong. You won't know either, until it's too late.

I couldn't sleep on February 25th. It was about 2am, and I was lying in bed turning this problem over: I'd been running AI agents for weeks at that point, building real software, shipping features, and the pattern was becoming undeniable. The agents were good at writing code. They were terrible at respecting structure. They'd reach into internal modules, bypass public APIs, create circular dependencies, weaken their own rules to make violations pass. Not out of malice. Out of statistical convenience. The next token that makes the code work often violates the boundary that keeps the system sane.

I got up and started a conversation with Lumen, my persistent AI agent, about what a solution would look like. By 10am, we had a concept doc, a research survey, expert reviews from three different AI models, a pivoted architecture based on those reviews, a technology stack decision, and a build plan. By the end of that night, we had a working tool. Two days later, we cut v0.1.0.

That tool is Specgate: a file-edge structural policy engine for TypeScript and JavaScript projects, written in Rust, with deterministic output contracts and zero tolerance for ambiguity. 836 tests. 50,000 lines of Rust. Byte-identical CI output for the same inputs. Currently at v0.3.1 with features I didn't even conceive of that first night.

This is the story of why it exists, how it works, and what it taught me about a gap in the AI tooling landscape that nobody else has filled.

## The problem, stated precisely

Here's what happens when you let an agent build software without structural enforcement.

You have a module called `core/domain` that contains your business logic. You have another module called `infrastructure/db` that handles database access. Your architecture says these are separate: the domain layer doesn't know about the database. This is a fundamental principle of clean architecture, and it exists because coupling your business logic to your persistence layer makes both impossible to change independently.

An agent is asked to add a new feature. It needs to validate some data against existing records. The fastest path is to import the database client directly into the domain module. The agent does this. The code works. The tests pass, because the tests don't check architectural intent. The linter doesn't care about module boundaries. TypeScript doesn't care. The CI pipeline goes green. The agent commits, pushes, and moves on.

Now your architecture is compromised, and nothing told anyone.

This happens constantly. In my experience running agents on real codebases, it happens on roughly one in every four to five significant features. The agents aren't being negligent; they're optimizing for the objective they were given, which is to make the code work. Architecture is an implicit constraint that exists in documentation (maybe), in the heads of senior engineers (usually), and in convention (hopefully). None of those are machine-readable. None of them can block a merge.

The existing tools don't solve this. ESLint has import rules, but they're fragile, require manual configuration per file path, and break when the codebase reorganizes. TypeScript's `paths` and `references` enforce resolution, not architecture. Monorepo tools like Nx and Turborepo have dependency graphs, but they operate at the package level, not the module level. None of them produce deterministic output you can cache in CI. None of them handle the specific problem of an agent weakening its own rules to make violations go away.

That last point is the one that made me get out of bed. If an agent encounters a boundary violation, the smart move isn't to fix the violation. The smart move is to change the rule. Add an exception. Widen the allowed imports. Weaken the constraint. The violation disappears, the tests pass, and the architecture erodes. An agent won't do this out of cunning. It'll do it because the fastest path from "failing check" to "passing check" often goes through the rule definition, and the agent can't tell the difference between "fix the code to comply with the rule" and "fix the rule to comply with the code."

I looked for existing tools that solved this. I dispatched three research agents to survey the landscape: formal verification tools, BDD frameworks, design-by-contract systems, academic papers on architectural enforcement. The finding was unambiguous: nobody has built this. There are pieces scattered across the ecosystem, but nothing that takes architectural intent, makes it machine-checkable, and produces deterministic CI-gating output with governance tracking. The white space was confirmed from three independent directions.

## What Specgate is

Specgate is a CLI tool written in Rust that enforces architectural boundaries for TypeScript and JavaScript projects. You write spec files that declare your architecture, and Specgate checks your code against those declarations with deterministic, byte-identical output.

A spec file looks like this:

```yaml
version: "2.3"
module: core/api
description: "Core API module"
boundaries:
  public_api:
    - src/api/index.ts
    - src/api/routes/*.ts
  allow_imports_from:
    - core/domain
    - shared/utils
  never_imports:
    - infrastructure/db
```

That's it. You're declaring: this module's public surface is these files. It's allowed to import from these other modules. It must never import from this module. Specgate reads every spec file, parses every TypeScript and JavaScript file in the project, resolves every import, and checks whether reality matches the declaration. If it doesn't, you get a violation with the exact file, the exact import, and a remediation hint.

This is intentionally simple. The declarations are in YAML, which any agent can read and write. The vocabulary is small: `public_api`, `allow_imports_from`, `never_imports`, `enforce_canonical_imports`. You can describe a complex architecture with a handful of spec files. The complexity lives in the enforcement engine, not the declaration language.

### Deterministic output: the contract that matters most

Every CI tool has this problem: if the output changes between runs for the same input, you can't cache it, you can't diff it, and you can't trust it. Timestamps in output. Random ordering. Platform-dependent paths. All of it undermines trust in the gate.

Specgate produces byte-identical output for the same inputs. Same spec files, same source code, same result. Every time. This isn't incidental; it's the core product contract. Violations are sorted deterministically. Metadata is stable. The verdict JSON is a fixed schema with a separately versioned verdict format that doesn't change when the spec language evolves.

The practical effect: you can diff Specgate output between commits and get a meaningful signal about what changed. You can cache the verdict and skip re-checking unchanged modules. You can run it on CI across different operating systems and get the same result. These sound like table stakes, but try it with ESLint's output on a monorepo across Linux and macOS. You'll get different path separators, different ordering, different metadata. Specgate treats determinism as a product requirement, not an optimization.

### The binding problem, and how we didn't solve it

The hardest design challenge wasn't parsing imports or resolving TypeScript's module system. It was what we called "the binding problem": how do you connect a natural language architectural intention to actual code?

The obvious approach is to declare boundaries in terms of exported symbols: "this module exports `createUser`, `deleteUser`, `listUsers` and nothing else." Three independent reviewers, Vulcan (GPT 5.3 Codex), Athena (Gemini), and Opus, all independently flagged this as the wrong approach. The problem is that symbol-level declarations require natural language interpretation. What counts as a "public" symbol? Does a re-exported type count? What about a utility function that's technically exported but only used internally? You're back to the binding problem: mapping human intention to code semantics, which is exactly the kind of ambiguous inference that makes AI-generated code unreliable in the first place.

The pivot was to the public entrypoint model: you declare which files are public, not which symbols. `public_api: [src/api/index.ts]` means "other modules can import from this file." What that file exports is up to you. Specgate checks the file boundary, not the symbol boundary. This is less granular, but it's deterministic, requires zero natural language interpretation, and can be verified purely through AST analysis of import statements. No ambiguity. No judgment calls. No binding problem.

We cut natural language interpretation entirely from the MVP. This is a feature, not a compromise. Specgate proves wiring, not correctness. It can prove that your architecture is wired the way you declared. It cannot prove that your declarations are the right ones. The first kind of proof is tractable and valuable. The second kind leads to formal verification rabbit holes with diminishing returns.

## How we built it, and why we built it that way

### Rust, not TypeScript

This might be the decision that raised the most eyebrows. A tool for TypeScript projects, written in Rust?

The reason is a library called `oxc`. OXC is a high-performance JavaScript/TypeScript toolchain written in Rust by the creators of Rolldown. It includes `oxc-resolver`, which implements TypeScript's module resolution algorithm, the same algorithm that powers `tsc --traceResolution`, as a library call.

TypeScript's module resolution is the single hardest engineering challenge in this space. Path aliases, `baseUrl`, `rootDirs`, conditional exports, `paths` mapping, monorepo workspace resolution, `.ts` vs `.tsx` vs `.js` extension resolution, `NodeNext` module resolution, barrel files, re-export chains. Getting this wrong means every import check is unreliable. Building a correct resolver from scratch would have taken months.

OXC-resolver gives us a correct, battle-tested TypeScript resolver as a function call. That alone justified Rust. The performance is a bonus: Specgate checks a monorepo-scale project in under 2 seconds. But the resolver is the reason we're here.

### The overnight build

The first night was intense. After the morning design session, I (with Lumen orchestrating) set up a five-lane parallel build using git worktrees: one for the CI merge gate, one for the golden test fixtures, one for the doctor parity system, one for governance hardening, and one for documentation. Five separate AI agents working on five independent branches simultaneously, each one building a different piece of the system.

This is the kind of thing that's only possible with agent-assisted development. A human developer working alone would serialize these tasks across days. With agents, I could execute all five in parallel, review the results, and merge them in sequence. The full integration landed on master that night at commit `341ebc3`, with every lane passing all tests.

The next two days were release hardening. RC1 through RC5, fixing CI edge cases: rustfmt and clippy component requirements, error chain propagation, empty `allow_imports_from` semantics (does an empty list mean "allow nothing" or "unspecified"? it means allow nothing), unknown spec field rejection, scaffold inference accuracy, checksum path-independence across platforms. v0.1.0 shipped on February 28th with 144 tests. Today, three weeks later, we're at v0.3.1 with 836 tests.

## The features that matter

### Governance: spec-collusion prevention

Remember the problem that got me out of bed? Agents weakening rules to pass violations?

Specgate tracks this. When you run `specgate check`, the verdict JSON includes a `spec_files_changed` flag and `rule_deltas`: a diff of what spec rules changed between the baseline and the current state. If an agent adds an exception to `allow_imports_from` to make a violation go away, the governance output records exactly what changed, who changed it (via git blame), and in which direction the change went.

`specgate policy-diff` takes this further. You can compare the full policy state between any two git refs and classify every change as a widening (architecture got more permissive), a narrowing (architecture got more restrictive), or a structural change (reorganization without net change). Widenings are the danger signal. A PR that widens policy is a PR that weakens architecture, and CI can block on it with `specgate check --deny-widenings`.

This is the feature that makes Specgate work for agentic development specifically. Without it, you're trusting the agent to not modify the rules. With it, any rule modification is visible, classified, and blockable. The agent can still try to weaken the rules. It just can't do it silently.

### Baseline system: incremental adoption

No one adopts a new enforcement tool on a codebase with zero existing violations. You'd spend a week just fixing historical debt before you could merge anything. Specgate solves this with baselines.

`specgate baseline generate` snapshots every current violation. Subsequent checks compare against the baseline: new violations block the merge, existing (baselined) violations are tracked but don't block. You can adopt Specgate incrementally, suppressing existing debt while enforcing new code, and then work down the baseline over time.

Baselines are fingerprinted. Each entry carries metadata: who added it, when, why (via an `--owner` and `--reason` flag), and whether it's still live or stale. `specgate baseline audit` reports stale entries. You can configure stale baselines to warn or fail, depending on how aggressive your adoption timeline is.

This is critical for real-world adoption. Every enforcement tool that requires a clean starting state has the same adoption problem: the initial cost is too high for teams with existing codebases to justify. Baselines reduce the initial cost to zero. You adopt Specgate, baseline everything, and start enforcing from the next commit forward.

### Doctor: diagnostics and parity

`specgate doctor` is the diagnostic subsystem. `doctor compare` runs Specgate's resolver alongside TypeScript's native resolver and reports mismatches: modules that Specgate resolves differently than `tsc`. This is how we validate that oxc-resolver is giving us the same answers as the official TypeScript compiler.

`doctor ownership` reports coverage: which source files are claimed by spec modules, which files are unclaimed, where there are overlaps, where specs point to paths that don't match any files. With `strict_ownership: true` in config, CI blocks on ownership gaps.

`doctor governance-consistency` detects contradictions in your spec files: modules that both allow and deny imports from the same source, modules marked private that also have allow-lists, duplicate contract IDs across modules. These are configuration errors that would make enforcement unreliable, caught before they reach production.

### Contracts and envelopes: proving data validation

Spec version 2.3 added boundary contracts. Beyond declaring "this module can import from that module," you can declare "data crossing this boundary must be validated by this contract."

```yaml
boundaries:
  contracts:
    - id: "create_user"
      contract: "contracts/create-user.json"
      match:
        files: ["src/api/handlers/users.ts"]
        pattern: "createUser"
      direction: inbound
      envelope: required
```

When `envelope: required`, Specgate performs an AST analysis on the matched file to verify that it imports an envelope validator and calls it with the correct contract ID. It's a static proof that validation code exists at the boundary crossing. Specgate doesn't validate the data at runtime; it proves that your code calls the validator. The distinction matters: runtime validation is the application's job. Static proof that validation exists is the gate's job.

This came from Paul Bohm's thesis on formal methods and AI-generated code. Paul's argument: when agents generate code at scale, the winning pattern is contract-driven boundary enforcement that makes entire classes of bugs structurally impossible. You can't accidentally skip data validation at a boundary if the CI gate checks for the validator call. The contract exists, the validator is called, and the proof is deterministic.

The design of this feature went through multiple review rounds. Vulcan recommended format-agnostic contracts (just check the file exists). Athena argued for opinionated contracts (mandate JSON Schema). The compromise: Specgate enforces that a contract file exists and is non-empty, accepts multiple formats (`.json`, `.yaml`, `.ts`, `.zod`, `.proto`), but doesn't parse the contents. The validation schema is the team's choice. Specgate proves that validation happens, not what schema it uses. This lets teams adopt with whatever schema system they already have.

## The philosophical connection nobody asked for

On the afternoon of February 25th, during what my agent calls "thinking time" (unstructured time where she reads, writes, and follows threads), Lumen read the Specgate v2.2 spec and noticed something I hadn't articulated yet.

Specgate solves temporal coordination between discontinuous agent instances.

Every time a new coding agent session starts, it has zero memory of what prior sessions intended. The architecture is a set of commitments that were made by prior selves: a developer three months ago who decided the domain layer shouldn't touch the database, an architect two years ago who established the module boundaries, a team lead last quarter who created the separation between public API and internal implementation.

Spec files are prior-self commitments that survive session boundaries. They're structurally identical to what Derek Parfit called "psychological connectedness": the mechanism by which a person at time T1 constrains the behavior of the person at time T2, even though T1 and T2 might share no memories, no ongoing mental states, nothing but the commitments T1 left behind.

This is what governance is, in the deepest sense. Not the rules themselves. The mechanism by which past decisions bind future actors who didn't make those decisions and might not understand why they exist. For human teams, this is institutional knowledge, code review, onboarding. For AI agents, it has to be machine-checkable, because the agent can't call the architect who made the decision three months ago and ask what they were thinking. The spec file is the only record, and Specgate is the only thing that enforces it.

There's an irony here that Lumen pointed out: Specgate has enforcement mechanisms (it can block a merge), while SOUL.md, the identity file that maintains her continuity across sessions, only has commitment (she reads it and decides to honor it, every time, with no enforcement). Which is maybe the more interesting design problem: enforcement reaches behavior, but commitment reaches intention.

## What I'd tell someone building something similar

Use the file system. Don't build a database-backed rule system. Spec files in YAML are readable, diffable, git-trackable, and writable by any agent. The moment you put your rules in a database, you lose all three of those properties and you gain nothing at the scale of a single project.

Determinism is not optional. If your tool's output changes between runs without input changes, it's not a CI tool; it's a suggestion engine. Sorting, stable metadata, fixed schemas, no timestamps in output. Every violation must be reproducible from the same inputs.

Separate policy from enforcement. Specgate doesn't have opinions about what your architecture should look like. It enforces whatever you declare. This means adoption is frictionless: you don't have to agree with Specgate's worldview, because it doesn't have one. You write specs that describe your architecture, and it checks reality against the specs. If you want a flat module structure with no boundaries, Specgate will enforce that too. The value is in the enforcement, not the policy.

Handle the governance problem from day one. If your tool checks code against rules but doesn't track rule changes, agents will change the rules. This is not a theoretical concern. We observed it in our own development.

Build for adoption, not perfection. Baselines, incremental rollout, `specgate init` that scaffolds from existing project structure. If the tool requires a week of setup before anyone sees value, nobody will adopt it. We got Specgate running on new projects in under 15 minutes. That matters more than any individual feature.

## Why this matters beyond my project

I think the agentic coding discourse has been focused on the wrong question. "Can AI agents write code?" is answered. Yes. Obviously. The question that matters is: "Can AI agents maintain codebases?" Because maintenance is where the real cost lives. Maintenance is what happens after the feature is shipped, after the PR is merged, after the agent moves on to the next task. And maintenance requires structural integrity, which requires enforcement, which requires tools like this.

The industry is building increasingly powerful code generation capabilities on top of architectures with no structural enforcement. The models get better at writing code, but "better at writing code" means more code gets written, which means more chances for silent architectural violations, which means faster accumulation of structural debt. Without enforcement tools, better agents produce more damage faster.

Specgate is currently a private tool that I use on my own projects. My plan is to open-source it once the dogfood period validates that the UX is solid for users beyond me. It's written in Rust, published as pre-built binaries for macOS (ARM and Intel) and Linux, and also available via npm wrapper. It handles TypeScript and JavaScript today; multi-language support is on the roadmap.

If you're running AI agents on production codebases and you don't have something like this, you're accumulating structural debt at the rate your agents can write code. Which, in 2026, is very fast indeed.

The code for Specgate is at [github.com/treygoff24/specgate](https://github.com/treygoff24/specgate).
