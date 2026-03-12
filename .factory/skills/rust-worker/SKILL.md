---
name: rust-worker
description: A worker that implements features and writes tests in the Specgate Rust codebase.
---

# Rust Worker

NOTE: Startup and cleanup are handled by `worker-base`. This skill defines the WORK PROCEDURE.

## When to Use This Skill

Use this skill when you need to implement a new feature, rule, or diagnostic tool in the Rust codebase (`src/` and `tests/`).

## Work Procedure

1. **Understand Requirements**: Read the feature description, expected behavior, and verification steps in `features.json`.
2. **Explore Code**: Use `rg` to find where the feature should be implemented. Understand the `Rule` trait if implementing a rule, or the `DoctorCommand` if implementing a diagnostic.
3. **Test-Driven Development (TDD)**:
   - For a core rule change: First, create a failing test in `tests/contract_fixtures.rs`.
   - For a full architectural rule: First, create failing physical fixtures in `tests/fixtures/golden/tier-a/`.
   - For diagnostics: Create programmatic temporary files or standard unit tests to mock CLI behavior.
4. **Implement**: Write the Rust code to make the tests pass. Ensure deterministic behavior (no incidental ordering changes).
5. **Verify**:
   - Run `cargo test contract_fixtures` to ensure core logic is intact.
   - Run `cargo test tier_a_golden` to verify architectural rule outputs.
   - Run `cargo clippy -- -D warnings` to enforce Rust standards.
   - Manually run the CLI against a local test fixture using `cargo run -- check --project-root <path>`.

## Example Handoff

```json
{
  "salientSummary": "Implemented C02 pattern-aware boundary rules in src/rules/boundary.rs. Added programmatic tests to tests/contract_fixtures.rs and verified they pass. Ran cargo clippy and ensured all deterministic output invariants hold.",
  "whatWasImplemented": "Added the PatternMatch struct to RuleContext. Modified the evaluate fn in src/rules/boundary.rs to check imports against defined patterns in the spec.yml file.",
  "whatWasLeftUndone": "",
  "verification": {
    "commandsRun": [
      {
        "command": "cargo test contract_fixtures -- --grep 'pattern_aware'",
        "exitCode": 0,
        "observation": "2/2 tests passed"
      },
      {
        "command": "cargo clippy -- -D warnings",
        "exitCode": 0,
        "observation": "No warnings found"
      }
    ],
    "interactiveChecks": [
      {
        "action": "cargo run -- check --project-root tests/fixtures/golden/tier-a/c02-pattern-aware/intro/",
        "observed": "CLI correctly reported pattern-matching violations matching the expected .verdict.json."
      }
    ]
  },
  "tests": {
    "added": [
      {
        "file": "tests/contract_fixtures.rs",
        "cases": [
          {
            "name": "test_c02_pattern_matching_violates",
            "verifies": "Ensures an import violating the pattern rule fails."
          },
          {
            "name": "test_c02_pattern_matching_passes",
            "verifies": "Ensures an import conforming to the pattern rule passes."
          }
        ]
      }
    ]
  },
  "discoveredIssues": []
}
```

## When to Return to Orchestrator

- You encounter an edge case not covered by the original requirements that fundamentally breaks the rule evaluation logic.
- You need to introduce a new dependency to `Cargo.toml` that hasn't been approved.
