# C02 Pattern-Aware Boundary

## Scenario
Module `app` declares `allow_imports_from: ["shared/*"]` using a glob pattern.
The `intro` variant imports from `legacy/old`, which does not match the pattern.
The `fix` variant removes the disallowed import, keeping only `shared/utils`.

## Target Rule
`boundary.allow_imports_from` (pattern-aware variant)

## Expected Behavior
- **INTRO:** FAIL — `app` imports from `legacy/old` which does not match `shared/*`
- **FIX:** PASS — `app` only imports from `shared/utils` which matches `shared/*`
