# Cross-repo Specgate Bug Archetypes (v2)

Scope mined:
- `/Users/treygoff/Development/openclaw-hud`
- `/Users/treygoff/Development/hearth`
- `/Users/treygoff/Development/treys-command-logic`

Constraint honored: no recency bias (older + newer examples included).

---

## Archetype 1) Contract shape drift (field name/type mismatches across module boundaries)

### Representative intro/fix SHA pairs

| Repo | Intro (bug introduced) | Fix | Evidence path(s) |
|---|---|---|---|
| openclaw-hud | `e9cd76f25724234022a1346dbc1c909e0888d069` | `bb4b36689999c7ccb59f4c330f22a924eb0f154b` | `public/app.js` |
| hearth | `d193850f7e6dd1e9ba12c0c3d6f2c49505f720a7` | `2cd1ef6d7a2a504884fe092ce02a9f8ff7c7969e` | `src/server/cronWatcher.ts` |
| treys-command-logic | `16320c44332157d67a9f1e6ae36db7cc1c74b325` | `9d26aff98817df5396455c13ead75d577dcf1924` | `src/hooks/useTeamMemberData.ts` |

### Shared failure mechanism
A producer/consumer boundary exists, but one side hardcodes stale or guessed schema:
- hud cron editor used `task`/`runTimeoutSeconds` while runtime contract expected `message|text`/`timeoutSeconds`.
- hearth watcher assumed cron `schedule` is `string`; gateway emits typed object (`{kind, expr/everyMs/at}`).
- TCL hook queried `team_members.created_at` while schema actually stores `joined_at`.

Net effect: silent filtering, empty UI, or runtime errors despite "valid" code.

### Specgate rule mapping
- **Primary:** `boundary.public_api` (only consume schema via canonical API/DTO modules)
- **Secondary:** `boundary.canonical_import` (force imports from canonical contract package, not ad-hoc local shape)
- **Supportive:** `enforce-layer` (UI -> adapter -> domain; direct UI <-> storage shape coupling disallowed)

### Reusable fixture template proposal
`fixtures/archetype-contract-shape-drift/`
- `contracts/cron.ts` (canonical schedule union)
- `watchers/cronWatcher.ts` (consumer)
- `ui/cronEditor.ts` (producer)
- `specgate/*.yml`:
  - consumers may import from `contracts/*`
  - consumers may **not** import storage/raw types directly
  - UI payload builders must import canonical contract types from one module

Failure test: duplicate local type or wrong key names in producer/consumer should trigger boundary/layer violation.

### Why common in AI-generated code
AI often pattern-matches nearby fields and "fills plausible names" (`task`, `created_at`) instead of tracing true source-of-truth contracts end-to-end.

---

## Archetype 2) Duplicate definitions after merge/feature accretion (shadowing + registry collisions)

### Representative intro/fix SHA pairs

| Repo | Intro (bug introduced) | Fix | Evidence path(s) |
|---|---|---|---|
| openclaw-hud | `8e78a621610714fd8f3b41c291550d66bd27da48` | `93baa0e030c9cb7586a63a46b441a1f5098163c2` | `public/utils.js` |
| hearth | `3076ac068a675a9d33b19e7d90387fb567cdd5c0` | `2b9d21a869188a879b288cc711c89c2786abc31e` | `src/shared/eventTypes.ts` |
| treys-command-logic | `6198e8a8ab1b973d0f2873c738599e84254995d7` | `5784d9e6e312b1d07b21cd0694d7a5dcb278f3f6` | `supabase/functions/claude-chat/tools/attachments.ts`, `supabase/functions/claude-chat/tools/notes.ts` |

### Shared failure mechanism
New feature landed without de-duplicating old definitions:
- hud had duplicate `wsUrl` key in object literal (JS last-write-wins shadowing changed behavior on HTTPS).
- hearth duplicated type aliases/events (`UsageProvider`, `UsageReport`, `UsageUpdateEvent`) and duplicated union entry.
- TCL added attachment tools while legacy placeholders with same tool names remained, crashing duplicate-name map initialization.

### Specgate rule mapping
- **Primary:** `no-circular-deps` + `enforce-layer` to discourage broad copy/paste module accretion across layers.
- **Custom-worthy extension:** `definition.unique_export` / `tool.unique_name` style rule (not current built-in) to enforce uniqueness in registries.
- **Boundary assist:** `boundary.public_api` to centralize registration in one module.

### Reusable fixture template proposal
`fixtures/archetype-duplicate-definition-collision/`
- `registry/index.ts` (single canonical registration surface)
- `legacy/*.ts` and `new/*.ts` each exporting same symbol/tool name
- spec asserts only `registry/*` may be imported by app entrypoints
- optional lint/validation step in fixture that fails on duplicate key/name set

Failure test: adding same symbol/tool in two files should fail immediately through fixture checks.

### Why common in AI-generated code
AI tends to append incremental patches in-place and preserve backward compatibility stubs, which can leave both old and new definitions active simultaneously.

---

## Archetype 3) External protocol/capability assumption drift (required/forbidden fields change)

### Representative intro/fix SHA pairs

| Repo | Intro (bug introduced) | Fix | Evidence path(s) |
|---|---|---|---|
| openclaw-hud | `3ba11b13b35f69822b2da1864d1cced75b953886` | `072477c0a8ebd413305cc97bc55ea012b9c0e408` | `lib/gateway-ws.js` |
| hearth | `b19749d6a486032e7198f17b9a823d7d90375c2c` | `567abbd7ed435367e5ab28f881d7f975e5d55dd7` | `src/server/index.ts` |
| treys-command-logic | `4a55a76b146309c431ce070420a0bdd30b5a12c6` | `f7ddfd30a547205995df50e13314811e5f7579d7` | `supabase/functions/claude-chat/providers/llm/anthropic/request.ts` |

### Shared failure mechanism
Upstream protocol semantics shifted (or were over-assumed):
- hud initially sent WS request frames without required `type:'req'`.
- hearth applied strict absent-origin rejection uniformly; valid HTTP server-to-server and top-level requests broke.
- TCL enabled `effort` parameter before API support, yielding provider rejection (`Extra inputs are not permitted`).

### Specgate rule mapping
- **Primary:** `boundary.public_api` (all external protocol calls must pass through a versioned adapter module).
- **Secondary:** `enforce-layer` (only adapter layer talks to network/provider SDKs; app/UI cannot construct protocol frames directly).
- **Dependency policy:** `dependency.not_allowed` to forbid direct imports of transport internals outside adapter modules.

### Reusable fixture template proposal
`fixtures/archetype-protocol-drift-adapter/`
- `adapters/gatewayClient.ts` + `adapters/providerClient.ts` (only protocol builders)
- `app/*` modules attempting direct frame/payload construction (expected to violate)
- golden contract tests with sample required/forbidden fields and version guards
- specgate constraints deny imports from `ws|sdk|transport` outside adapters

Failure test: direct protocol object construction outside adapter should fail spec + fixture.

### Why common in AI-generated code
AI optimizes for immediate compatibility with local examples, not stable protocol lifecycle management; it often bakes assumptions instead of isolating them in versioned adapters.

---

## Notes for main synthesis
- These three archetypes are strongly cross-cutting across all 3 repos.
- The most actionable Specgate gain is enforcing **adapter boundaries + canonical contract imports**; this would have prevented a large fraction of these intro commits from compiling/merging cleanly.
