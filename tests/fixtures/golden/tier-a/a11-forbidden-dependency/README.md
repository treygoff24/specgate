# A11: forbidden-dependency

- **Tier:** A (P0)
- **Target rule:** `dependency.forbidden`
- **Gate status:** Validated in golden corpus (`d01-forbidden-third-party`) because this fixture requires npm dependency graph materialization.

Intro imports a forbidden third-party package.
Fix removes the forbidden import.
