# MVP Merge Gate

**One operator-facing definition of “safe to merge” for Specgate.**

This document turns plan section 15(1) into a copy/paste CI gate.

---

## What the MVP Gate Must Prove

A change is merge-ready when all four checks are run and interpreted consistently:

1. **Wave 0 contract stability**
   - `cargo test contract_fixtures`
   - Proves locked CLI/version semantics still hold.
2. **Tier A deterministic rule gate**
   - `cargo test tier_a_golden`
   - Proves intro/fix exactness and deterministic ordering for P0 fixtures.
3. **Golden corpus safety signal**
   - `cargo test golden_corpus`
   - Tracks broader behavior classes and regression signal.
4. **Real project policy check with baseline/new split**
   - `specgate check --output-mode deterministic`
   - Proves operator-facing CI behavior (`baseline_hits` vs `new_violations`).

---

## First Working CI Sequence (Copy/Paste)

```bash
# 1) Contract lock
cargo test contract_fixtures

# 2) Deterministic merge gate
cargo test tier_a_golden

# 3) Broader regression signal
cargo test golden_corpus

# 4) Repository policy check (byte-identical output mode)
./target/release/specgate check --output-mode deterministic
```

For PR performance, use blast radius:

```bash
./target/release/specgate check --since origin/main --output-mode deterministic
```

---

## Failure Reason Mapping (Required for CI Clarity)

Map failures into one of these buckets:

- **Contract drift**
  - Trigger: `contract_fixtures` or `tier_a_golden` fails
  - Meaning: semantic contract changed or determinism regressed
- **Policy failure**
  - Trigger: `specgate check` exits `1`
  - Meaning: new policy violations were introduced
- **Runtime/setup failure**
  - Trigger: `specgate check` exits `2` or test harness/runtime error
  - Meaning: tooling/config/environment issue, not policy semantics

---

## Baseline Behavior in Gate Decisions

Use baseline to suppress known debt while still blocking new risk:

- `baseline_hits` = known violations (reported)
- `new_violations` = newly introduced violations (merge-blocking)

Generate/update baseline:

```bash
./target/release/specgate baseline --write .specgate-baseline.json
```

---

## Relationship to Other Docs

- [Operator Guide](OPERATOR_GUIDE.md) — onboarding narrative
- [CI Gate Understanding](CI-GATE-UNDERSTANDING.md) — detailed CI patterns
- [Wave 0 Contract](../WAVE0_CONTRACT.md) — locked semantic surface
- [Tier A Fixture Design](tier-a-fixture-design-v1.md) — deterministic gate spec
- [Implementation Plan §15](specgate-implementation-plan-v1.1.md#15-remaining-work-prioritized) — roadmap context
