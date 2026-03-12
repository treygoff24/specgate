# User Testing Guide for Specgate

## Testing Tool
CLI testing via direct `cargo run` commands. No browser or TUI automation needed — Specgate is a pure CLI tool.

## Surfaces
- Terminal output of `specgate check` and `specgate doctor` commands.

## Setup
- `cargo build` to compile the project.
- No external services, databases, or APIs required.
- Test fixtures live in `tests/fixtures/`.

## How to Run a Check
```bash
cargo run -- check --project-root <fixture-path> --format json --no-baseline
```
- `--format json` ensures machine-parseable output.
- `--no-baseline` prevents baseline classification from interfering with raw violation output.

## Key Fixture Locations
- **C02 Pattern-Aware:** `tests/fixtures/golden/tier-a/c02-pattern-aware/{intro,fix}/`
- **C06 Category-Gov:** `tests/fixtures/golden/tier-a/c06-category-gov/{intro,fix}/`
- **C07 Unique-Export:** `tests/fixtures/golden/tier-a/c07-unique-export/{intro,fix}/`

Each fixture has:
- `intro/` — violating variant (expect FAIL)
- `fix/` — corrected variant (expect PASS)
- `expected/intro.verdict.json` and `expected/fix.verdict.json` — expected output shape

## Expected Verification Pattern
For each assertion, run the CLI against both `intro` and `fix` variants:
1. `intro` should produce violations (status: fail)
2. `fix` should produce no violations (status: pass)

## Flow Validator Guidance: CLI
- No shared state between CLI runs — each run is against a separate fixture directory.
- No isolation concerns — CLI reads fixtures read-only.
- Parallel execution is safe.
- Always use `--format json` for machine-parseable output.
- Always use `--no-baseline` to get raw violation counts.
