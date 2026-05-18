# OpenClaw HUD — Specgate Golden-Corpus Bug Hunt (v2)

Repo analyzed: `/Users/treygoff/Development/openclaw-hud`  
Date: 2026-02-25 (CST)

## Method used (per request)
1. Synced history/context:
   - `git fetch --all --prune`
   - `git branch -vv`, `git status --branch`
2. Mined deep history (not recency-biased):
   - `git log --all --grep='fix|bug|regression|revert|hotfix'`
   - targeted archaeology with `git log -S ...`, `git blame`, `git show`
3. Selected only strongest, high-signal introducing→fix pairs with clear causality.

---

## 1) Wrong semantic API for “New Chat”: `sessions_spawn` used instead of `sessions.reset`

- **Introducing SHA:** `935974cae165b8dc31bace8756d6c6a83f1550be`
- **Fixing SHA:** `432bb3101647669bf49e3a17bc28740e7c735c37`

### Files touched
- **Introducing commit files (relevant):**
  - `ws/chat-handlers.js`
  - `tests/ws/chat-handlers.test.js`
  - `lib/gateway-ws.js`
  - `server.js`
- **Fix commit files:**
  - `ws/chat-handlers.js`
  - `public/chat-input.js`
  - `public/chat-ws-handler.js`
  - `tests/ws/chat-handlers.test.js`

### Failure pattern + root cause
- **Pattern:** action-to-API semantic mismatch.
- “New chat” in top-level HUD should reset the current implicit session, but implementation invoked `tools/invoke` with `sessions_spawn` (subagent/session creation semantics).
- Root cause: LLM-coded flow used superficially plausible API (`spawn`) without modeling product/domain semantics (top-level chat lifecycle ≠ child session lifecycle).

### Exact Specgate rule mapping
- `constraints.rule: no-pattern`
  - **Rule intent:** ban `sessions_spawn` in `chat-new` path.
  - **Example mapping snippet:**
    ```yaml
    constraints:
      - rule: no-pattern
        params:
          pattern: "SwitchCase[test.value='chat-new'] ObjectProperty[key.name='tool'][value.value='sessions_spawn']"
        severity: error
    ```
- Optional companion (if/when supported): require pattern for reset call in same path.

### Minimal fixture proposal (8 files)
1. `ws/chat-handlers.js` (contains `chat-new` switch path)
2. `public/chat-input.js` (emits `chat-new` message)
3. `public/chat-ws-handler.js` (handles `chat-new-result`)
4. `lib/gateway-ws.js` (request abstraction)
5. `tests/ws/chat-handlers.test.js`
6. `spec/chat-new-api-contract.spec.yml`
7. `specgate.config.yml`
8. `README-fixture.md` (expected fail/pass behavior)

### Why this is high-signal for AI-generated code
- Classic “looks right syntactically, wrong operationally” error.
- Exactly the kind of semantic tool/API misuse LLMs produce under partial context.
- Easy to regress during autonomous refactors.

### Confidence
- **0.95 (very high)** — blame/diff lineage is explicit and fix commit message directly states semantic mismatch.

---

## 2) Duplicate object key regression: two `wsUrl` definitions, HTTPS downgraded to `ws://`

- **Introducing SHA:** `8e78a621610714fd8f3b41c291550d66bd27da48`
- **Fixing SHA:** `93baa0e030c9cb7586a63a46b441a1f5098163c2`

### Files touched
- **Introducing commit files (relevant):**
  - `public/utils.js`
  - `public/chat-pane.js`
  - `public/panels/session-tree.js`
  - `public/panels/sessions.js`
  - `tests/public/chat-pane.test.js`
- **Fix commit files:**
  - `public/utils.js`
  - `public/app.js`
  - `public/panels/session-tree.js`
  - `public/panels/sessions.js`

### Failure pattern + root cause
- **Pattern:** merge-style duplicate symbol in object literal (silent overwrite).
- Two `wsUrl` keys existed in `HUD.utils`; second definition overwrote first (JS object semantics).
- Caller passed a pseudo-location object with `protocol: 'wss:'`; surviving function expected `https:` check, fell through to `ws://`.
- Production effect: incorrect websocket scheme under HTTPS pages.

### Exact Specgate rule mapping
- `constraints.rule: no-pattern`
  - **Rule intent:** forbid duplicate property keys in object literals for critical utility objects.
  - **Example mapping snippet:**
    ```yaml
    constraints:
      - rule: no-pattern
        params:
          pattern: "ObjectExpression:has(Property[key.name='wsUrl'] ~ Property[key.name='wsUrl'])"
        severity: error
    ```
- Optional broader rule: forbid duplicate keys in any exported utility object.

### Minimal fixture proposal (7 files)
1. `public/utils.js` (duplicate-key case)
2. `public/app.js` (caller with ws URL construction)
3. `public/chat-pane.js` (minimal consumer presence)
4. `tests/public/app.test.js`
5. `tests/public/utils.test.js`
6. `spec/no-duplicate-object-keys.spec.yml`
7. `specgate.config.yml`

### Why this is high-signal for AI-generated code
- LLM merge/regeneration frequently introduces duplicate members when reconciling variants.
- Silent overwrite bugs are high-severity and low-visibility (no syntax error, runtime misbehavior only).

### Confidence
- **0.96 (very high)** — fix commit explicitly documents overwrite mechanism and HTTPS downgrade chain.

---

## 3) Reconnect logic dead after first successful WS open

- **Introducing SHA:** `e344a8fc33773c923e355c9ce2ebfb7907dd9dbd`
- **Fixing SHA:** `c3b4df1ecaae4d9b07e92499c3208717c5746bb5`

### Files touched
- **Introducing commit files (relevant):**
  - `public/app.js` (introduces `wsEverOpened` guard + conditional reconnect scheduling)
  - `tests/public/app.test.js`
- **Fix commit files:**
  - `public/app.js`
  - `public/chat-pane.js`
  - `tests/public/app.test.js`

### Failure pattern + root cause
- **Pattern:** state-flag gating that blocks recovery path.
- `scheduleWsReconnect` returned early when `wsEverOpened` was true.
- `onclose/onerror` only scheduled reconnect before first successful open (`!opened` path).
- After one successful connection, later disconnects never reconnected; queued chat requests timed out.

### Exact Specgate rule mapping
- `constraints.rule: no-pattern`
  - **Rule intent:** forbid reconnect schedulers that gate on “ever opened” state.
  - **Example mapping snippet:**
    ```yaml
    constraints:
      - rule: no-pattern
        params:
          pattern: "FunctionDeclaration[id.name='scheduleWsReconnect'] IfStatement[test.value=/wsEverOpened.*\|\|/ ]"
        severity: error
    ```
- Optional companion pattern: forbid `onclose` reconnect guarded by `if (!opened)` in persistent channels.

### Minimal fixture proposal (8 files)
1. `public/app.js` (ws lifecycle + reconnect logic)
2. `public/chat-pane.js` (queue/send path depending on socket health)
3. `public/utils.js` (ws URL helper)
4. `tests/public/app.test.js` (disconnect/reconnect behavior)
5. `tests/public/chat-pane.test.js` (queue-drain behavior)
6. `spec/ws-reconnect-liveness.spec.yml`
7. `specgate.config.yml`
8. `README-fixture.md`

### Why this is high-signal for AI-generated code
- LLMs often add boolean “safety” guards that look reasonable locally but violate liveness globally.
- This is an archetypal agent error: local defensive coding causing distributed/system-level dead behavior.

### Confidence
- **0.92 (high)** — introducing and fixing diffs align directly; fix commit explains exact failure mechanics.

---

## Final selection rationale
These three are the strongest because they each have:
- clear introducing SHA and explicit fixing SHA,
- concrete runtime impact,
- deterministic static signature suitable for Specgate corpus fixtures,
- and high relevance to common AI-agent failure modes (semantic API misuse, merge artifact overwrite, liveness guard regression).