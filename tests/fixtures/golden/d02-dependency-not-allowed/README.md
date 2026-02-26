# D02: dependency-not-allowed

- **Tier:** B (P1) - deterministic, catchable-now
- **Target rule:** `dependency.not_allowed`

Intro imports a third-party package (`axios`) that is not in the allowed dependencies list.
Fix uses only the allowed packages (`fetch` is native, no external deps needed).