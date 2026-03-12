# C07: Unique Export Visibility Enforcement

## Scenario
Two files within the same module boundary both export the same name (`getData`),
violating the `boundary.unique_export` constraint.

## Intro (fail)
- `src/tools/alpha.ts` exports `getData` and `alpha`
- `src/tools/beta.ts` exports `getData` and `beta`
- Module declares `boundary.unique_export` constraint
- Specgate detects the duplicate `getData` export → 1 violation

## Fix (pass)
- `src/tools/beta.ts` renames its export to `getBetaData`
- No more duplicate exports → 0 violations
