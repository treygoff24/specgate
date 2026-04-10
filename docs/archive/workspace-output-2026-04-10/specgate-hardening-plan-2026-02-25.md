# Specgate MVP Hardening Plan (Post-Phase-3)

Date: 2026-02-25  
Branch context: `feature/phase3-integration-glm` (Phase 3 complete, tests green)

## Goal
Move Specgate from “strong engine” to “trusted product gate” by eliminating false-confidence paths and aligning shipped semantics with the original problem statement.

## Success Criteria (Definition of Ready-to-Dogfood)
- Semantic contract is explicit and matches implementation.
- No known high-impact false-positive/false-negative policy paths.
- Git-aware CI workflow is unambiguous and tested.
- Ignore/suppression governance is enforceable and auditable.
- Golden corpus demonstrates real catches on real agent bug classes.
- Doctor/debug UX is sufficient for rapid trust calibration.

---

## Wave A — Contract Lock + Trust-Critical Corrections (must-do first)

### A1. Lock CLI semantics
- Decide and document final meaning for:
  - `--since <git-ref>` (git blast-radius mode)
  - `--baseline-diff` (baseline comparison output)
- Reserve `--diff` only if meaning is crystal clear; avoid overloaded semantics.

**Acceptance:** CLI help/docs/tests all reflect one unambiguous contract.

### A2. Boundary semantics correctness
- Ensure `allow_imports_from` / `never_imports` support intended pattern semantics.
- Make `public_api` semantics module-relative and deterministic (e.g., provider module entrypoint resolution).

**Acceptance:** targeted fixtures pass for exact/pattern/public-api cases.

### A3. Escape hatch governance enforcement
- Enforce required ignore reason.
- Enforce expiry behavior and `require_expiry`.
- Enforce new-ignore caps in diff-aware workflows.
- Ensure suppression behavior is consistent across rule families.

**Acceptance:** governance fixtures verify hard failure/warning paths exactly as designed.

### A4. No silent PASS on blind spots
- Surface parse failures/unresolved dynamic imports in verdict (at minimum warnings; optional strict CI fail-closed mode).

**Acceptance:** intentionally broken fixtures cannot silently PASS.

---

## Wave B — Product Credibility via Real-World Evidence

### B1. Golden corpus v1 (minimum 5 real cases)
- Add 5 real agent-generated failure reproductions from actual repos/work.
- For each case: source pattern, intended policy, expected violation.

### B2. Golden corpus v2 (target 10+)
- Expand to all key failure classes in MVP problem statement.

**Acceptance:** running corpus shows deterministic catches with stable outputs.

---

## Wave C — Operator UX + Adoption Hardening

### C1. Doctor UX
- Ensure `doctor <file>` explains:
  - module ownership
  - import resolution chain
  - why a specific rule fired
  - what to change next

### C2. Fix hints and guidance
- Add actionable `fix_hint` per major violation type.

### C3. Init/docs polish
- Ensure scaffold defaults are safe and realistic.
- Align docs/spec language with shipped semantics (no hidden drift).

**Acceptance:** new team member can install, init, run, interpret, and fix violations without source-diving.

---

## Suggested Execution Order (Fast Path)
1. **Wave A (all items)**
2. **Golden corpus v1 (5 cases)**
3. **Frontier holistic re-review**
4. **Golden corpus v2 + UX polish**

---

## Risk Register
- **R1: False confidence in CI gates** → mitigated by A3/A4 + golden corpus.
- **R2: Semantic ambiguity** → mitigated by A1 + docs lock.
- **R3: Adoption friction** → mitigated by C1/C2/C3.

---

## Final Merge Gate for “MVP Ready” Claim
All must be true:
- Wave A complete and tested.
- Golden corpus v1 complete and passing.
- Independent frontier review finds no unresolved high-impact semantic risks.
