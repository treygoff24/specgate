# Adding a New Rule to Specgate

When implementing a new constraint rule (like enforce-layer, enforce-category, boundary.unique_export), these integration points must all be touched:

1. **Rule module**: Create `src/rules/<rule_name>.rs` with types for Config, Violation, ConfigIssue, Report, plus `parse` and `evaluate` functions.
2. **Module re-export**: Add `pub mod <rule_name>;` to `src/rules/mod.rs`.
3. **Analysis pipeline**: Wire the rule into `src/cli/analysis.rs` — call parse/evaluate, map violations to `PolicyViolation`, and propagate config issues to `AnalysisArtifacts`.
4. **CLI types**: Add the rule's artifact fields to `src/cli/types.rs` (AnalysisArtifacts struct).
5. **Doctor output**: Add fields to `src/cli/doctor/types.rs` (DoctorOutput) and include in `src/cli/doctor/overview.rs` summary rendering.
6. **Spec validation**: Register the rule's constraint key in `KNOWN_CONSTRAINT_RULES` in `src/spec/validation.rs`.

## Conventions

- Use `BTreeMap`/`BTreeSet` for deterministic iteration order.
- Sort violations and config issues before returning for CI-safe output.
- Follow the single-canonical-config pattern: when multiple modules declare the same rule, the lexicographically first module ID wins (see enforce-layer, enforce-category).
- Include unit tests in the rule module, contract fixture E2E tests in `tests/contract_fixtures.rs`, and a Tier A golden fixture with intro/fix variants in `tests/fixtures/golden/tier-a/`.
