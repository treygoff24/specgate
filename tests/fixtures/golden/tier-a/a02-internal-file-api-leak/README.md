# A02: internal-file-api-leak

- **Tier:** A (P0)
- **Maps from Tier B:** C09
- **Target rule:** `boundary.public_api`

Intro imports provider internals outside `public_api`.
Fix imports provider public entrypoint.
