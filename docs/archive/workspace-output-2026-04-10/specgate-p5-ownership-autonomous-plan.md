# Specgate P5 — Ownership Registry Improvements (autonomous plan)

## Scope for this slice
Take the next numbered Specgate backlog slice after P2: **P5 Ownership Registry Improvements**.

Implement the highest-value ownership gaps that are still open, grounded in the current codebase:
- [x] Confirm what already exists vs. what is truly missing in ownership validation/doctor output
- [x] Create a feature branch from `master` for this slice
- [x] Implement ownership diagnostics foundation with TDD
- [x] Add/expand tests for overlaps, orphaned specs, and unclaimed files
- [x] Add a focused ownership doctor surface (`specgate doctor ownership`) or equivalent explicit ownership diagnostic UX if that is cleaner with current architecture
- [x] Evaluate `strict_ownership` config support; defer it if it would overreach this slice
- [x] Update docs/changelog for any new ownership diagnostics behavior
- [x] Run full verification: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `./scripts/ci/mvp_gate.sh`
- [x] Run independent code review pass(es) on the finished diff
- [x] Address any review findings, then re-run verification/review as needed
- [x] Merge the finished branch into `master` only after all quality gates are green
- [x] Push the merge to `origin/master`

## Constraints
- TDD required: failing tests first, then implementation, then green
- Clean code, minimal surface area, no speculative backlog work beyond this slice
- Preserve deterministic output conventions
- Prefer extending existing ownership/resolver/doctor structures over parallel abstractions
- If the cleanest result is a narrower sub-slice than full P5, choose the smallest shippable slice and document the remaining backlog explicitly

## Verification command
`cd /Users/treygoff/Development/specgate && /Users/treygoff/.cargo/bin/cargo fmt --check && /Users/treygoff/.cargo/bin/cargo clippy --all-targets -- -D warnings && /Users/treygoff/.cargo/bin/cargo test && ./scripts/ci/mvp_gate.sh`
