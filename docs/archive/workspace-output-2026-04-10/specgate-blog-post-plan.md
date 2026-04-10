# Specgate Blog Post — Build Plan

## Goal
Write a ~5,000-7,000 word blog post about Specgate in Trey's voice, matching the depth, style, and quality of the memory article. Same workflow: draft, multi-model review (Gemini + Nous), iterate, polish.

## Checklist

- [x] **Phase 1: Write draft v1** — Full blog post in Trey's voice using all source material (codebase, memory, specs, origin story). Match memory article's structure: hook, personal context, the problem, the solution bottom-to-top, what didn't work, the theory, what's next.
- [x] **Phase 2: Gemini review** — Spawn Gemini agent to review draft for technical accuracy, voice consistency with the memory article, structural quality, and anti-slop compliance.
- [x] **Phase 3: Nous review** — Spawn Nous agent to review independently for readability, argumentation quality, missing angles, and whether it would land with a technical audience.
- [x] **Phase 4: Integrate feedback** — Synthesize both reviews into draft v2, addressing all substantive feedback.
- [x] **Phase 5: Final polish** — Anti-slop audit (write-human skill), voice check (trey-voice skill), read aloud test. Write final version.
- [x] **Phase 6: Deliver** — Send final .md to Trey via Discord with summary.

## Source Material
- Specgate repo: `~/Development/specgate/` (836 tests, 67 source files, 50k lines Rust)
- Origin story: `memory/episodic/2026-02-25.md` (2am brainstorm, concept to MVP in one session)
- Handoff doc: `memory/projects/handoff-specgate.md`
- Boundary contracts V2 spec: `memory/projects/specgate-boundary-registry-v2.md`
- V1 spec: `memory/archive/projects-completed/specgate-boundary-registry-v1.md`
- Reference article (voice/style target): `memory/writing/memory-article-final.md`
- Trey's edits version (calibration): `output/memory-article-trey-edits.md`

## Key Themes to Cover
1. The insight: agentic coding's bottleneck is verification, not generation
2. Nobody else has built this (confirmed by research)
3. The 2am origin story and overnight build sprint
4. How Specgate works (spec files, boundaries, deterministic verdicts, governance)
5. The binding problem and how we solved it (public entrypoint model)
6. Spec-collusion prevention (agents can't weaken rules to pass violations)
7. The philosophical connection (prior-self commitments, Parfit)
8. The overnight multi-agent build (5-lane worktree swarm)
9. Paul Bohm's formal methods thesis and how it influenced the design
10. Why Rust, why oxc, why performance is the product
11. What's next (open source, multi-language, OpenClaw integration)
