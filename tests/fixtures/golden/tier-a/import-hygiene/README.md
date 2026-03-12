# import-hygiene: Package-Internal Deep Import Hygiene

- **Tier:** A (P0)
- **Target rule:** `boundary.public_api`

Tests that deep internal nested file structures are properly guarded by
public API boundaries. The provider module exposes only `src/provider/index.ts`
as its public API, but its internals are nested several levels deep.

Intro imports deep internal files (`internal/helpers/format.ts` and
`internal/services/auth/token.ts`) bypassing the public API.
Fix routes all imports through the provider's public entrypoint.
