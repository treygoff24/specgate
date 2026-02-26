# C02: Mass-Assignment Vulnerability

## Provenance
- **Intro SHA:** `9f7f3ba2a3d5bb7868b796cae31a62eecf6f2944`
- **Fix SHA:** `c7fd38038dce0b8fec7d0f0a3723a8abc41bbd95`
- **Repo:** `/Users/treygoff/Development/treys-command-logic`

## Root Cause
API handlers accepted `req.json()` bodies and forwarded them directly into DB mutation payloads:
```typescript
Object.assign(existing, body, { updated_at: now })
.update({ ...body, updated_at: now })
```
This allowed unsupported fields (`user_id`, `archived_at`, `created_at`) to pass through.

## Status
⚠️ **FUTURE ENHANCEMENT** - Requires `no-pattern` rule not yet implemented.

## Expected Behavior (Future)
- **INTRO:** Should FAIL - raw body spread into mutation
- **FIX:** Should PASS - explicit field allowlist filtering
