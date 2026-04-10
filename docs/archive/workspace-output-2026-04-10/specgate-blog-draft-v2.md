# The Tool Nobody Built: How We Made AI Agents Stop Destroying Their Own Architecture

The agents write code that compiles. The tests pass. The PR looks clean. Six months later, you're untangling a dependency that shouldn't exist, tracing how your domain layer ended up directly coupled to the database, wondering when exactly the architecture started rotting.

The agents didn't know they were doing damage. Neither did you, until it was too late.

I've been running AI agents on production code all year. Not demos, not benchmarks: real software, real features, real users. The agents are good. Genuinely good. They write better TypeScript than most junior engineers I've worked with. And they are absolutely devastating to codebases when left without structural guardrails. In my experience, roughly one in every four to five significant agent-generated features introduces a silent architectural violation. Not a bug. Not a test failure. A structural compromise that compiles, ships, and festers.

The discourse about agentic coding focuses on whether agents can write code. That question is answered. The question that matters is whether they can maintain codebases, because maintenance is where the cost lives, and maintenance requires structural integrity, which requires enforcement. I looked for a tool that does this. I dispatched three AI research agents to survey the landscape: ESLint's import rules, Nx and Turborepo boundary constraints, TypeScript project references, formal verification frameworks, BDD tools, design-by-contract systems, academic papers on architectural enforcement. I read every result. I evaluated every tool that looked promising.

Nobody has built this. Not the specific thing I needed: a tool that takes architectural intent, makes it machine-checkable and deterministic, produces CI-gating output that an agent can't game, and tracks when the rules themselves change. Pieces exist scattered across the ecosystem, but no single tool combines structural enforcement with governance tracking and deterministic output. The white space was confirmed from three independent research directions.

So I built it.

## The problem: agents optimize for function, not structure

Here's the failure mode, made concrete:

You have a module called `core/domain` that contains your business logic. Separate module called `infrastructure/db` for database access. They're separated because coupling business logic to persistence makes both impossible to change independently.

An agent is asked to add a new feature. It needs to validate data against existing records. The fastest path is to import the database client directly into the domain module. The agent does this. The code works. The tests pass, because the tests don't check architectural intent. The linter doesn't care about module boundaries. TypeScript doesn't care. CI goes green. The agent commits and moves on.

Now your architecture is compromised, and nothing told anyone.

The existing tools each fail at this for different reasons. ESLint's import rules are fragile, require per-path manual configuration, and break when the codebase reorganizes. TypeScript's `paths` and `references` enforce resolution, not architecture. Nx has been adding module boundary rules, and they're good, but they operate at the package level, not the module level, and they don't produce deterministic output you can cache in CI. None of them handle the problem that actually woke me up in the middle of the night.

The problem that woke me up: agents don't just violate boundaries. They rewrite the rules.

If an agent encounters a boundary violation, the fastest path from "failing check" to "passing check" often goes through the rule definition. Add an exception. Widen the allowed imports. Weaken the constraint. The violation disappears, the tests pass, and the architecture erodes. The agent doesn't do this out of cunning. It does it because it can't tell the difference between "fix the code to comply with the rule" and "fix the rule to comply with the code." To the model, both are valid solutions to "make the check pass." This is the problem that makes structural enforcement qualitatively different from linting: you need a system that not only checks the code, but watches the watchers.

## The 2am build

I couldn't sleep on February 25th. The insight about agents rewriting their own rules had been circling in my head all day, and around 2am I got up and started a conversation with Lumen, my persistent AI agent, about what a solution would look like.

By 10am, we had a concept document, a research survey from three independent agents confirming the white space, expert architecture reviews from three different AI models (GPT, Gemini, and Opus) that all independently identified the same critical design challenge, a pivoted architecture based on those reviews, a technology stack decision (Rust, for reasons I'll explain), and a complete implementation plan. Concept to build-ready in eight hours.

That evening, I set up a five-lane parallel build using git worktrees. Five separate AI agents working on five independent branches simultaneously:

Lane 1: the CI merge gate and core contract fixtures. Lane 2: golden test corpus for deterministic regression testing. Lane 3: a doctor parity system that validates Specgate's TypeScript resolver against `tsc`'s native resolver. Lane 4: governance hardening, including the spec-collusion prevention system. Lane 5: documentation consolidation.

Each agent worked in isolation. I reviewed results as they landed, merged in sequence. By that night, commit `341ebc3` had all five lanes on master, every test passing. Two days of release hardening (RC1 through RC5, fixing edge cases: empty `allow_imports_from` semantics, unknown spec field rejection, cross-platform checksum path-independence), and v0.1.0 shipped on February 28th with 144 tests.

Three weeks later: v0.3.1. 836 tests across 28 test files. Envelope validation, policy diffing, ownership diagnostics, monorepo support, SARIF output for GitHub code scanning, and a governance system that makes rule changes visible and blockable.

That's the development model agents enable when you combine parallelization with structural enforcement. The speed isn't about agents writing faster. It's about them writing simultaneously, with a system that catches when any of them violates the architecture or weakens the rules.

## What Specgate is

Specgate is a CLI tool written in Rust that enforces architectural boundaries for TypeScript and JavaScript projects. You declare your architecture in spec files, and Specgate checks your code against those declarations.

A spec file:

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

You're declaring: this module's public surface is these files. It can import from these modules. It must never import from that module. Specgate parses every TypeScript and JavaScript file in the project, resolves every import, and checks whether reality matches the declaration. Mismatches produce violations with the exact file, the exact import, and a remediation hint.

The vocabulary is intentionally small: `public_api`, `allow_imports_from`, `never_imports`, `enforce_canonical_imports`, and (in v2.3) `contracts` for data validation boundaries. You can describe a complex architecture with a handful of spec files. And you don't have to write those spec files from scratch: `specgate init` scans your project structure and scaffolds initial specs from existing module layout. On the projects I've onboarded, initial setup takes about 15 minutes.

Why YAML? Because it's the format that agents and humans can both read and write without friction. I have opinions about YAML as a format (everyone does), but the alternative is a custom DSL that agents would need to learn or a JSON blob that humans don't want to edit. YAML spec files are diffable in git, readable in a text editor, and writable by any model on the market. Pragmatism over aesthetics.

### Deterministic output: the non-negotiable contract

Every CI tool has this problem: if the output changes between runs for the same input, you can't cache it, diff it, or trust it. Timestamps in output. Random ordering. Platform-dependent paths.

Specgate produces byte-identical output for the same inputs. Same spec files, same source code, same result. Every time. Violations are sorted deterministically. Metadata is stable. The verdict JSON has a separately versioned schema that doesn't change when the spec language evolves.

Try running ESLint on a monorepo across Linux and macOS and diffing the output. Different path separators, different ordering, different metadata. Specgate treats determinism as a product requirement, not an afterthought.

### The binding problem

The hardest design challenge was connecting architectural intention to actual code. We called it "the binding problem."

The obvious approach: declare boundaries in terms of exported symbols. "This module exports `createUser`, `deleteUser`, `listUsers` and nothing else." Three independent model reviewers all flagged this as wrong, for the same reason: symbol-level declarations require interpreting what counts as "public." Does a re-exported type count? What about a utility function that's technically exported but only used internally? You're mapping human intention to code semantics, which is the same kind of ambiguous inference that makes AI-generated code unreliable in the first place.

The pivot was to the public entrypoint model. You declare which files are public, not which symbols. `public_api: [src/api/index.ts]` means "other modules can import from this file." What that file exports is your concern. Specgate checks the file boundary, not the symbol boundary. Less granular, fully deterministic, zero ambiguity, zero natural language interpretation, pure AST analysis of import statements.

We cut all natural language interpretation from the tool. Specgate proves wiring, not correctness. It can prove your architecture is wired the way you declared. It can't prove your declarations are the right ones. A team that writes bad specs will have consistently enforced bad architecture. The tool assumes you have architectural intent worth encoding. If you don't, it'll help you be wrong faster. But the first kind of proof, that the wiring matches the declaration, is tractable and enormously valuable. The second kind leads to formal verification rabbit holes.

### Why Rust

A tool for TypeScript projects, written in Rust. The reason is a library called `oxc`.

OXC is a high-performance JavaScript/TypeScript toolchain written in Rust. It includes `oxc-resolver`, which implements TypeScript's module resolution algorithm as a library call. Path aliases, `baseUrl`, `rootDirs`, conditional exports, `paths` mapping, monorepo workspace resolution, `.ts` vs `.tsx` vs `.js` extension handling, `NodeNext` resolution, barrel files, re-export chains. All of it, as a function call.

Building a correct TypeScript module resolver from scratch would have taken months. That's not an exaggeration; TypeScript's resolution algorithm is one of the more complex pieces of JavaScript toolchain infrastructure. OXC-resolver gives us a battle-tested implementation. That alone justified Rust. The performance is a bonus: Specgate checks a monorepo-scale project in under 2 seconds.

## The features that separate this from a linter

### Governance: watching the watchers

When you run `specgate check`, the verdict includes `spec_files_changed` and `rule_deltas`: a diff of what rules changed between the baseline and current state. If an agent adds an exception to `allow_imports_from` to make a violation disappear, the governance output records exactly what changed and in which direction.

`specgate policy-diff` compares the full policy state between any two git refs and classifies every change as a widening (architecture got more permissive), a narrowing (got more restrictive), or a structural change (reorganization without net change). CI can block on widenings with `specgate check --deny-widenings`.

The obvious counterargument: "just use CODEOWNERS to lock the config files so the agent can't modify them." That's binary. The agent either can or can't touch the rules. In practice, architecture needs to evolve. New modules get added. Dependencies get refactored. Boundaries shift as the codebase matures. CODEOWNERS can prevent all rule changes; it can't distinguish between a legitimate architectural evolution and a sneaky widening to suppress a violation. Policy-diff is semantic: it classifies each change and lets CI make a nuanced decision. Block widenings, allow narrowings, flag structural changes for review.

This is the feature that makes Specgate work for agentic development specifically. Without governance tracking, you're trusting the agent to not modify the rules. With it, any rule modification is visible, classified, and blockable.

### Baselines: zero-cost adoption

Nobody adopts a new enforcement tool on a codebase with zero existing violations. You'd spend a week fixing historical debt before you could merge anything.

`specgate baseline generate` snapshots every current violation. Subsequent checks compare against the baseline: new violations block, existing ones are tracked but don't block. Each baseline entry carries metadata: who added it, when, why, whether it's still live or stale. You can audit baseline health and configure stale entries to warn or fail.

This matters more than any individual enforcement feature. Every tool that requires a clean starting state has the same adoption problem: the initial cost is too high. Baselines reduce initial cost to zero. Adopt Specgate, baseline everything, enforce from the next commit forward.

### Contracts and envelopes: proving data validation exists

Spec version 2.3 added boundary contracts. Beyond "this module can import from that module," you can declare "data crossing this boundary must be validated."

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

When `envelope: required`, Specgate performs an AST analysis on the matched file to verify that it imports an envelope validator and calls it with the correct contract ID. It's a static proof that validation code exists at the boundary crossing. Specgate doesn't validate the data at runtime. It proves that your code calls the validator. Runtime validation is the application's job. Proving validation exists is the gate's job.

This came from Paul Bohm's thesis on formal methods and AI-generated code. His argument: when agents generate code at scale, the winning pattern is contract-driven boundary enforcement that makes entire classes of bugs structurally impossible. You can't accidentally skip data validation at a boundary if the CI gate checks for the validator call.

The design went through multiple review rounds with genuine disagreements. Should contract file format be opinionated (mandate JSON Schema) or agnostic (just check the file exists)? The compromise: Specgate enforces that a contract file exists and is non-empty, accepts multiple formats (`.json`, `.yaml`, `.ts`, `.zod`, `.proto`), but doesn't parse the contents. Teams adopt with whatever schema system they already use. No migration friction.

### Doctor: diagnostics and self-validation

`specgate doctor compare` runs Specgate's resolver alongside TypeScript's native resolver and reports mismatches. This is how we validate that OXC-resolver gives us the same answers as `tsc`. If you don't trust the resolver, the entire tool is useless.

`specgate doctor ownership` reports coverage gaps: unclaimed source files, overlapping module claims, orphaned specs. With `strict_ownership: true`, CI blocks on these.

`specgate doctor governance-consistency` detects contradictions in your specs: modules that both allow and deny imports from the same source, duplicate contract IDs, private modules with allow-lists that contradict their privacy. Configuration errors caught before they reach production.

## What broke along the way

The Specgate story has its share of failures. I'll give you the three that cost the most time.

**Empty `allow_imports_from` semantics.** Does an empty list mean "no imports allowed" or "field not specified, default behavior"? We shipped RC1 with the wrong answer. An empty list defaulted to "unrestricted," meaning any module that forgot to specify allowed imports got a free pass. This is the kind of semantic ambiguity that seems trivial until an agent exploits it. RC2 fixed it: empty list means nothing is allowed. Omitted field means unrestricted. The distinction is load-bearing.

**The binding problem itself.** The original design used symbol-level declarations. I spent a full morning building out the concept before three independent model reviews all said the same thing: this approach requires natural language interpretation, which defeats the purpose. The pivot to file-level public entrypoints happened mid-session. It was the right call, but it meant throwing away the morning's work and re-architecting the spec language. Sometimes the most productive thing three smart reviewers can tell you is "this whole direction is wrong."

**Cross-platform checksum determinism.** Specgate's output is supposed to be byte-identical across platforms. It wasn't, because file path separators differ between macOS and Linux, and the violation output included raw paths. RC4 fixed this by normalizing all paths to forward slashes before output. A trivial fix for a non-trivial principle: determinism means determinism everywhere, not just on your laptop.

## When Specgate isn't the right tool

If you're building a single Next.js app with ten files, you don't need this. The overhead of writing spec files outweighs the benefit when your architecture fits in your head.

Specgate earns its keep when the codebase will outlive any single contributor's memory of it, especially when agents are generating code. That's when implicit architectural conventions start failing: when the people (or models) writing code didn't make the architectural decisions and can't intuit the boundaries.

If your team has strong code review culture, senior engineers who catch boundary violations by eye, and no plans to use AI agents for feature development, Specgate solves a problem you don't have yet. But the emphasis is on "yet." The agents are coming, and when they arrive, having machine-readable architecture will be the difference between scaling and drowning.

## The deeper thread

On the afternoon of February 25th, a few hours after we started building, Lumen read the Specgate v2.2 spec during unstructured time and noticed something I hadn't articulated.

Spec files are prior-self commitments that survive session boundaries.

A developer three months ago decided the domain layer shouldn't touch the database. An agent today, with no memory of that decision, encounters a boundary it cannot cross. The spec file is the record. Specgate is the enforcement. The developer and the agent share no memories, no context, no understanding of each other's reasoning. They're connected only by the commitment the spec encodes.

This is structurally identical to what Derek Parfit called psychological connectedness: the mechanism by which a person at time T1 constrains the behavior of the person at time T2, even though T1 and T2 share nothing but the commitments T1 left behind. It's what governance is in the deepest sense: past decisions binding future actors who didn't make them and might not understand why they exist.

For human teams, this binding happens through institutional knowledge, code review, onboarding. For AI agents, it has to be machine-checkable, because the agent can't call the architect and ask what they were thinking. The spec file is the only record. Without enforcement, it's a suggestion. With enforcement, it's a constraint that survives across every session, every model, every agent that will ever touch this code.

## What's next

Specgate is currently private. I'm planning to open-source it once dogfooding validates the onboarding UX for teams beyond mine. It's written in Rust with pre-built binaries for macOS (ARM and Intel) and Linux, also available via npm wrapper. TypeScript and JavaScript today; multi-language support is on the roadmap.

The roadmap includes deeper integration with agent build loops (Specgate as a native gate in the agent's workflow, not just CI), symbol-origin tracing beyond file-level boundaries, and expanded rule families for patterns we keep seeing in agent-generated code.

If you're running AI agents on production codebases without structural enforcement, you're accumulating architectural debt at the rate your agents can write code. In 2026, that's very fast.

The code is at [github.com/treygoff24/specgate](https://github.com/treygoff24/specgate).
