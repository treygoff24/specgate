# Specgate Governance/Blind-Spot Bug Hunt (v2)

Scope analyzed (read-only git archaeology):
- `/Users/treygoff/Development/openclaw-hud`
- `/Users/treygoff/Development/hearth`
- `/Users/treygoff/Development/treys-command-logic`

Goal: find high-signal slips tied to weak checks / blind spots / architectural shortcuts, with concrete intro+fix SHAs and fixture recommendations.

---

## Case 1 — Auth bypass via conflicting mode flags (weak check precedence)
**Repo:** `hearth`

- **Intro SHA:** `a30fdca629d0a4427154220c1afd860eb8a7d762`
- **Fix SHA:** `90c76f9a3c0ac0bb59cfe3aae22e2b56a3f1ba30`

### Files touched
- **Intro commit touched:**
  - `src/server/index.ts`
  - `src/server/sessionEndpoint.ts`
  - `src/server/ticketStore.ts`
  - `src/server/index.test.ts`
  - `src/server/sessionEndpoint.test.ts`
  - (plus client auth wiring files)
- **Fix commit touched:**
  - `src/server/index.ts`
  - `src/server/index.test.ts`
  - `src/server/sessionEndpoint.ts`
  - `src/server/sessionEndpoint.test.ts`
  - `src/client/constants/config.ts`
  - `src/client/constants/config.test.ts`

### Bypass / blind-spot mechanism
`a30fdca` introduced ticket auth, but WS upgrade guard was:
- `if (!DEV_ALLOW_UNAUTH && ticketStore) { ...ticket validation... }`

If both `HEARTH_CLIENT_TOKEN` **and** `HEARTH_DEV_ALLOW_UNAUTH=true` were set, ticket validation was skipped even though token mode existed. That created an auth bypass via conflicting config states.

`90c76f9` fixed this by enforcing ticket checks whenever `ticketStore` exists (token configured), and warning on dual-flag config.

### Specgate control that should catch/prevent this
- **Fail-closed config invariant control** (blind-spot control, A4-style):
  - When secure mode is configured, insecure override flags cannot disable enforcement.
  - Treat contradictory security-mode states as a governance violation (or at least hard warning with CI option to fail).

### Minimal fixture proposal
- **Fixture:** tiny server config matrix test.
- Inputs:
  1. `CLIENT_TOKEN=on`, `DEV_ALLOW_UNAUTH=false` → WS upgrade without ticket must fail.
  2. `CLIENT_TOKEN=on`, `DEV_ALLOW_UNAUTH=true` → WS upgrade without ticket must still fail.
  3. `CLIENT_TOKEN=off`, `DEV_ALLOW_UNAUTH=true` → allowed in explicit dev mode.
- Assert: mode precedence + explicit warning for contradictory flags.

**Confidence:** **High** (fix commit message explicitly calls out this high-severity bypass; code path matches).

---

## Case 2 — Canonicalization fallback fabricated valid-looking IDs (unresolvable-path blind spot)
**Repo:** `openclaw-hud`

- **Intro SHA:** `1f12e15a2be48bb238874153b6644b1c86775021`
- **Fix SHA:** `a66437ac47e9db7b662f8e8d52426d9d284921d6`

### Files touched
- **Intro commit touched:**
  - `routes/agents.js`
  - `routes/sessions.js`
  - `lib/helpers.js`
  - `tests/routes/agents.test.js`
  - `tests/routes/sessions.test.js`
- **Fix commit touched (relevant):**
  - `routes/agents.js`
  - `lib/helpers.js`
  - `tests/public/panels/agents.test.js`
  - (plus unrelated UI-QoL files in same commit)

### Bypass / blind-spot mechanism
`1f12e15` added:
- `safeCanonicalizeSessionKey()` which catches canonicalization errors and returns synthetic `agent:${agentId}:${key}`.

That converted invalid/unresolvable session keys into plausible canonical keys instead of failing/excluding them, i.e., a silent fallback path around strict `canonicalizeSessionKey` validation.

`a66437a` removed this bypass: on canonicalization error, the item is dropped (reduce/skip), not fabricated.

### Specgate control that should catch/prevent this
- **Unresolvable reference visibility control** (A4 “no silent PASS on blind spots”):
  - If canonicalization/resolution fails, do not synthesize valid-looking fallback identifiers.
  - Surface unresolved references in verdict/governance output; optionally fail-closed in CI mode.

### Minimal fixture proposal
- **Fixture:** session inventory with malformed key samples (e.g., invalid chars / wrong canonical agent prefix).
- Assert:
  - malformed entries are reported as unresolved,
  - not silently normalized into canonical IDs,
  - strict mode fails build on unresolved count > 0.

**Confidence:** **High** (intro+fix diffs are direct and localized to the bypass helper).

---

## Case 3 — Duplicate tool contracts crashed runtime (architectural migration shortcut)
**Repo:** `treys-command-logic`

- **Intro SHA:** `6198e8a8d0ef6d3d8620fb4f84012221bba5c2a3`
- **Fix SHA:** `5784d9e6e312b1d07b21cd0694d7a5dcb278f3f6`

### Files touched
- **Intro commit touched:**
  - `supabase/functions/claude-chat/tools/attachments.ts` (new)
  - `supabase/functions/claude-chat/tools/index.ts`
  - `supabase/functions/claude-chat/tool-execution/handlers/attachments.ts`
  - `supabase/functions/claude-chat/tool-execution/index.ts`
  - `supabase/functions/claude-chat/tool-execution/input-types.ts`
  - `supabase/functions/claude-chat/tools/shared.ts`
- **Fix commit touched:**
  - `supabase/functions/claude-chat/tools/notes.ts`

### Bypass / blind-spot mechanism
`6198e8a` introduced `attachments.ts` tool definitions while `notes.ts` still contained legacy attachment placeholders (`list_attachments`, `attach_file`, `delete_attachment`, etc.).

`tools/index.ts` already throws on duplicate names (`Duplicate tool definition detected...`), so this became a startup crash path (before normal request handling). The blind spot was governance/CI: migration introduced overlapping contract IDs without a dedicated preflight guardrail/fixture preventing merge.

`5784d9e` removed the legacy duplicates from `notes.ts`.

### Specgate control that should catch/prevent this
- **Contract-collision governance control**:
  - enforce global uniqueness of externally exposed contract IDs (tool names/API contract IDs) at verification time,
  - include rule/API-surface delta governance to flag risky overlaps during migrations.

### Minimal fixture proposal
- **Fixture:** two modules each defining same contract ID (`attach_file`).
- Assert:
  - verification fails with deterministic collision report naming both provider files,
  - governance output includes `rule_deltas/spec_files_changed` (or equivalent) for reviewer visibility.

**Confidence:** **High** (commit history + fix message explicitly identifies duplicate-tool startup crash cause).

---

## Summary
Strongest signals found:
1. **Weak check precedence in security mode config** (`hearth`): contradictory flags disabled auth enforcement.
2. **Silent fallback over unresolved canonicalization** (`openclaw-hud`): invalid keys converted to valid-looking IDs.
3. **Migration-time contract collisions** (`treys-command-logic`): duplicate tool IDs caused runtime crash.

These map cleanly to Specgate hardening themes:
- fail-closed invariants for sensitive paths,
- no silent pass on unresolved/blind-spot states,
- governance checks for migration-time contract/rule collisions.
