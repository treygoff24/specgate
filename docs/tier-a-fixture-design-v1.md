# Tier A Fixture Design v1 (Catchable-Now CI Gate)

## Objective

Define a Tier A fixture set that is:
1. Catchable by current rules now,
2. Deterministic and CI-safe,
3. Product-useful (guards real failure classes),
4. Distinct from Tier B future/proxy corpus.

Tier B remains valuable roadmap evidence, but Tier A is the merge gate.

---

## Tier A Contract (Non-negotiable)

A fixture is Tier A only if:
- **Intro fails now** on an implemented rule.
- **Fix passes now** under same config.
- **Exact expected violations** match (not “contains”).
- **Single-intent** (one target rule; no accidental extra rule failures).
- **Deterministic ordering** verified across repeated runs.

### Exact pass/fail semantics
For each fixture variant (`intro`, `fix`):

- Command: `specgate check --project-root <variant-root> --no-baseline`
- Intro must assert:
  - `exit_code == 1`
  - `violations.len == expected_count` (default 1)
  - exact `rule_id`, `from_module`, `to_module` (or exact expected set)
  - `unexpected_rule_ids == ∅`
  - `analyzer_errors == ∅`
- Fix must assert:
  - `exit_code == 0`
  - `violations.len == 0`

### Determinism contract
- Run intro **3 times**.
- Compare normalized violation records byte-for-byte, sorted by:
  `rule_id, from_module, to_module, from_file, to_file`.
- Exclude unstable fields (absolute paths, timestamps).

---

## Proposed Tier A Fixtures

## P0 (Gate immediately)

### A01 — ingress-persistence-bypass
- **Rule:** `boundary.allow_imports_from`
- **Intro:** ingress imports infra/db directly.
- **Fix:** ingress imports domain façade.
- **Why:** catchable-now structural guardrail for mass-assignment-style pathways.

### A02 — internal-file-api-leak
- **Rule:** `boundary.public_api`
- **Intro:** consumer imports provider internal file not in `public_api`.
- **Fix:** import provider public entrypoint.
- **Why:** enforces explicit module contract boundary now.

### A03 — layer-reversal-origin-guard
- **Rule:** `enforce-layer`
- **Intro:** forbidden cross-layer edge per declared order.
- **Fix:** move shared logic to allowed layer.
- **Why:** catchable-now architecture inversion guard.

### A04 — registry-canonical-entrypoint
- **Rule:** `boundary.canonical_import`
- **Intro:** cross-module relative import into provider with canonical import enforcement enabled.
- **Fix:** canonical import ID.
- **Why:** hardens single-entrypoint contract and anti-bypass behavior.

### A06 — external-cycle-registry
- **Rule:** `no-circular-deps` (scope: external)
- **Intro:** create cross-module import cycle.
- **Fix:** break cycle via interface/facade split.
- **Why:** covers graph-level catchable rule missing from current candidate set.

## P1 (Add after P0 stabilizes)

### A07 — provider-visibility-private
- **Rule:** `boundary.visibility.private`
- **Intro:** consumer imports from module marked as private visibility.
- **Fix:** removes import; respects private boundary.
- **Why:** enforces hard module isolation for internal/private APIs.

### A08 — provider-visibility-internal
- **Rule:** `boundary.visibility.internal`
- **Intro:** non-friend module imports from internal-visibility provider.
- **Fix:** routes through friend module or avoids import.
- **Why:** guards internal APIs from unauthorized cross-team/module access.

### A09 — importer-never-imports
- **Rule:** `boundary.never_imports`
- **Intro:** importer imports from module it declared as never-importable.
- **Fix:** removes forbidden import.
- **Why:** explicit importer-side deny list for strict module boundaries.

### A10 — provider-deny-imported-by
- **Rule:** `boundary.deny_imported_by`
- **Intro:** module imports from provider that explicitly denies it.
- **Fix:** routes through allowed intermediary or avoids import.
- **Why:** explicit provider-side deny list for access control.

**Note:** A11 (forbidden-dependency) was created but excluded from Tier A gate because it requires npm dependencies (package.json + node_modules), breaking the deterministic, self-contained Tier A criteria. Dependency rules are tested by D01/D02 in regular golden corpus.

### A05 — forbidden-dependency-in-sensitive-module
- **Rule:** `dependency.forbidden` (single pinned rule; no dual-rule ambiguity)
- **Intro:** sensitive module imports forbidden dependency (use deterministic resolvable target, e.g. `node:fs`, or explicit local package fixture).
- **Fix:** remove/replace dependency.
- **Why:** useful governance control, but lower confidence as a Tier A starter than P0 set.

---

## Anti-gaming requirements

- Reject fixtures containing `@specgate-ignore` in intro/fix source.
- Add at least one **near-miss** check (looks similar but should pass) for precision confidence (recommended for A01 or A05).
- Keep each fixture isolated so only its target rule fires.

---

## Fixture structure and location

Use immutable roots to avoid runtime mutation:

- `tests/fixtures/golden/tier-a/a01-ingress-persistence-bypass/intro/...`
- `tests/fixtures/golden/tier-a/a01-ingress-persistence-bypass/fix/...`
- ... same for A02, A03, A04, A06 (and A05 in P1)

Each fixture includes:
- `README.md` (intent + provenance + rule mapping)
- `fixture.meta.yml` with:
  - `id`, `tier`, `maps_from_tier_b`, `target_rule`, `expected_count`
- `specgate.config.yml`
- `modules/*.spec.yml`
- `expected/intro.verdict.json`
- `expected/fix.verdict.json`

Test harness:
- `tests/tier_a_golden.rs` (required CI gate)
- `tests/golden_corpus.rs` stays Tier B (informative/non-gating).

---

## Tier B → Tier A migration mapping

- `C02 -> A01`
- `C09 -> A02`
- `C08 -> A03`
- `C07 -> A04` (+ A06 for cycle hardening)
- `C06 -> A05` (category-level governance, P1)

Promotion rule:
- Tier B case moves to Tier A only after intro/fix become deterministic exact-contract fixtures on current engine behavior.
