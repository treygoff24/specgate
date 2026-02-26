# D01: forbidden-third-party-dependency

- **Tier:** B (P1) - deterministic, catchable-now
- **Target rule:** `dependency.forbidden`

Intro imports a third-party package (`lodash`) that is explicitly forbidden in the spec.
Fix removes the forbidden dependency and uses native alternatives.