# A01: ingress-persistence-bypass

- **Tier:** A (P0)
- **Maps from Tier B:** C02
- **Target rule:** `boundary.allow_imports_from`

Intro intentionally routes ingress directly to persistence (`infra/db`), bypassing domain.
Fix restores ingress → domain façade.
Near-miss demonstrates a type-only carve-out that should pass.
