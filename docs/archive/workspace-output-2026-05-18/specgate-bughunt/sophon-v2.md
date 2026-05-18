# Specgate Golden-Corpus Bug Hunt — Sophon v2

Repo scanned: `/Users/treygoff/Development/treys-command-logic`

Prioritized deeper history and high-signal intro→fix pairs with clean Specgate mapping.

---

## 1) Unauthenticated service-role cron endpoint (critical auth bypass)

- **Intro SHA:** `1cc1caa102b4318786f8da9191c9053910dfa301`
- **Fix SHA:** `f378201dd972da90d4e64d06a76b758c022aa453`

### Files touched

**Intro (key):**
- `supabase/functions/auto-archive-completed/index.ts` (new file)
- `supabase/functions/claude-chat/index.ts`
- `supabase/migrations/20250812110000_add_archiving.sql`
- `src/pages/Projects.tsx`

**Fix (key):**
- `supabase/functions/auto-archive-completed/index.ts`
- `supabase/functions/claude-chat/index.ts`
- `supabase/migrations/20250812110000_add_archiving.sql`
- `src/pages/Projects.tsx`

### What broke and why

The new edge function used a **service-role client** and executed archive actions for all users, but had no bearer-token/service-key gate in the initial implementation.

- Intro had permissive flow (`serve(async (req) => { ... profiles.select(...) ... autoArchiveForUser(...) })`) with no auth check.
- Fix added explicit authorization checks:
  - `authorization` header required
  - token must equal `SUPABASE_SERVICE_ROLE_KEY`
- Fix also tightened CORS behavior and added stronger validation/error handling around archiving paths.

### Specgate rule mapping

- **Auth Guard Required on privileged endpoints** (edge functions using service role must enforce explicit auth)
- **No unauthenticated bulk mutation path**
- **CORS hardening on privileged endpoints**

### Minimal fixture shape

- Deno/Supabase edge function
- `createClient(SUPABASE_URL, SUPABASE_SERVICE_ROLE_KEY)`
- `serve(async (req) => { ... mutate DB ... })`
- Missing `Authorization` validation before mutation

### Confidence

**High.** Commit message explicitly calls this a critical auth bypass; diff shows direct introduction and direct fix in same file with unambiguous security guard insertion.

---

## 2) Tool registry crash from duplicate tool names (schema collision)

- **Intro SHA:** `6198e8a8ab1b973d0f2873c738599e84254995d7`
- **Fix SHA:** `5784d9e6e312b1d07b21cd0694d7a5dcb278f3f6`

### Files touched

**Intro (key):**
- `supabase/functions/claude-chat/tools/attachments.ts` (new)
- `supabase/functions/claude-chat/tools/index.ts`
- `supabase/functions/claude-chat/tool-execution/handlers/attachments.ts`
- `supabase/functions/claude-chat/tool-execution/index.ts`

**Fix (key):**
- `supabase/functions/claude-chat/tools/notes.ts`

### What broke and why

`tools/index.ts` builds `TOOL_DEFINITIONS` then throws if duplicate names are present:

- `if (map.has(tool.name)) throw new Error(...)`

Intro added `attachmentTools` into the global list, but legacy placeholder attachment-like tools still existed in `notes.ts` (`list_attachments`, `attach_file`, `delete_attachment`, etc.).

Result: startup-time duplicate-name exception before normal request handling.

Fix removed stale duplicate definitions from `notes.ts`.

### Specgate rule mapping

- **Global uniqueness of tool names across all tool modules**
- **No duplicate contract IDs in registry assembly**
- **Fail-fast preflight check on generated tool schema set**

### Minimal fixture shape

- Multi-file tool registry:
  - `toolsA = [{name:'list_attachments'}]`
  - `toolsB = [{name:'list_attachments'}]`
- Registry initializer with duplicate-name throw
- Intro event = adding second tool module to ordered list without dedupe/removal

### Confidence

**High.** Intro commit introduces attachment tool module + inclusion in ordered list; fix commit explicitly removes duplicate tool definitions and references startup crash in message.

---

## 3) Mass-assignment + weak payload validation in API v1 PATCH/POST handlers

- **Intro SHA:** `9f7f3ba2a3d5bb7868b796cae31a62eecf6f2944`
- **Fix SHA:** `c7fd38038dce0b8fec7d0f0a3723a8abc41bbd95`

### Files touched

**Intro (key):**
- `supabase/functions/api-v1/handlers/tasks.ts`
- `supabase/functions/api-v1/handlers/projects.ts`
- `supabase/functions/api-v1/handlers/notes.ts`

**Fix (key):**
- `supabase/functions/api-v1/handlers/tasks.ts`
- `supabase/functions/api-v1/handlers/projects.ts`
- `supabase/functions/api-v1/handlers/notes.ts`
- `supabase/functions/api-v1/utils.ts`
- `supabase/functions/api-v1/api-v1.test.ts`

### What broke and why

Intro introduced modular handlers that accepted `req.json()` bodies and forwarded them directly into mutation payloads:

- `Object.assign(existing, body, { updated_at: now })`
- `.update({ ...body, updated_at: now })`

This allows unsupported keys (e.g., `user_id`, `archived_at`, `created_at`) to pass through and weakly typed values to hit DB paths.

Fix added:
- allowlists per endpoint (`*_CREATE_FIELDS`, `*_PATCH_FIELDS`)
- `unsupportedFields(...)` rejection with `INVALID_FIELDS`
- stronger type/date validation helpers
- tests explicitly asserting mass-assignment rejection and payload validation errors

### Specgate rule mapping

- **PATCH/POST field allowlist required (no spread of raw body into DB update)**
- **Reject unknown fields with deterministic error code**
- **Type/format validation before persistence**

### Minimal fixture shape

- Request handler with:
  - `const body = await req.json()`
  - direct `.update({ ...body })` or `Object.assign(entity, body)`
- No explicit allowed-fields filter
- Test payload containing forbidden fields (`user_id`, `created_at`, `archived_at`)

### Confidence

**High.** `-S` history cleanly links intro and fix around the exact vulnerable pattern; fix includes regression tests that directly encode the bug class.

---

## Why these 3 are strongest for Specgate corpus

1. **Clear causal intro→fix linkage** (same core files, explicit bug language)
2. **Machine-detectable patterns** (auth guard absence, duplicate IDs, mass-assignment spread)
3. **Reusable fixture shapes** that generalize well to static/spec checks
4. Mix of **security + reliability + contract/schema integrity** failures
