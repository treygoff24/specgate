# C06 Category-Level Governance

## Scenario
Modules `auth/login` and `billing/invoices` belong to isolated domain categories.
The `intro` variant has `auth/login` importing from `billing/invoices`, violating
the category isolation policy. The `fix` variant removes the cross-category import,
keeping only an import from `shared/utils` (which is outside the governed categories).

## Target Rule
`enforce-category` (category-level isolation governance)

## Expected Behavior
- **INTRO:** FAIL — `auth/login` imports from `billing/invoices` which crosses a category boundary within the "domains" category set
- **FIX:** PASS — `auth/login` only imports from `shared/utils` which is not a governed category member
