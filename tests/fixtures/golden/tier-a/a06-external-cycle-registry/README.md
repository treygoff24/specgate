# A06: external-cycle-registry

- **Tier:** A (P0)
- **Maps from Tier B:** C07 extension
- **Target rule:** `no-circular-deps` (scope: external)

Intro creates a cross-module cycle between `registry` and `worker`.
Fix breaks the cycle while preserving collaboration.
