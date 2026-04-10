# Specgate Phase 5: Envelope AST Check — Build Checklist

**Branch:** `phase5/envelope-ast-check`
**Repo:** `~/Development/specgate`
**Starting state:** master @ f613a81, 420 tests passing

---

## Tasks

- [x] **T1: Envelope Config** — Add `EnvelopeConfig` struct to `src/spec/config.rs` with `enabled`, `import_patterns`, `function_pattern` fields. Add to `SpecConfig`. Tests for defaults, overrides, YAML round-trip, `enabled: false`.
- [x] **T2: Envelope AST Analyzer** — Create `src/rules/envelope.rs` module. Targeted second AST pass using oxc_parser on specific files. Detect envelope imports (skip type-only), extract call expressions matching function_pattern with contract ID from first arg (StringLiteral, TemplateLiteral, TSAsExpression unwrap). Handle destructured imports, renamed imports, CJS require, optional chaining. Unit tests for all patterns.
- [x] **T3: match.pattern Function Scoping** — In `src/rules/envelope.rs`, add `find_function_span()` to locate exported function matching pattern name. Scope envelope call search to that function's AST subtree. Tests for scoped pass/fail, arrow functions, missing function.
- [x] **T4: Integration + Severity Fix** — Add `severity` field to `ContractRuleViolation`. Refactor `check_match_patterns()` to return resolved paths. Add `check_envelope()` inside `evaluate_contract_rules()` loop. Fix `analyze_project()` severity wiring in `src/cli/mod.rs`. Wire into verdict builder.
- [x] **T5: Test Fixtures** — Create 18 fixture directories under `tests/fixtures/envelope/`. Write `tests/envelope_checks.rs` with integration tests for each scenario. Verify verdict JSON structure, severity, human output format.
- [x] **T6: Documentation** — Update `docs/reference/spec-language.md`, `docs/reference/getting-started.md`. Create `docs/reference/envelope-guide.md`. Update `docs/design/boundary-contracts-v2.md` "What This Proves" table. Add CHANGELOG entry.
- [x] **Final: Gate** — All tests pass. `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && ./scripts/ci/mvp_gate.sh` green. No regressions on existing 420 tests.
