# Specgate golden-corpus bug hunt: Hearth (v2)

Repo: `/Users/treygoff/Development/hearth`

Selection principle applied: **quality over recency**; searched full available history and prioritized examples with explicit AI-agent provenance (commit footers / review notes).

---

## 1) Public API leakage: internal server object exposed across module boundary

- **Intro SHA:** `89d79b1cdb3434a92d64484146e54691e6b27856` (Co-Authored-By: Claude Opus 4.6)
- **Fix SHA:** `cbd164298efc65168ea25f265f0a4bec64af2ad1` (Co-Authored-By: Claude Opus 4.6)

### Files touched
- **Intro:**
  - `src/server/webhookIngress.ts`
  - `src/server/webhookIngress.test.ts`
  - `src/server/index.ts`
  - `src/server/index.test.ts`
  - (plus related plumbing/docs)
- **Fix:**
  - `src/server/webhookIngress.ts`
  - `src/server/webhookIngress.test.ts`
  - `src/server/index.ts`
  - `src/server/index.test.ts`

### Failure pattern + root cause
`createWebhookIngress()` returned `{ server, close }`, leaking Node’s raw `http.Server` object outside ingress boundary. This widened the public surface and allowed callers to bypass intended façade methods.

Fix tightened API to `{ close: () => Promise<void>, address: () => Server['address'] }`, removing direct `server` escape hatch.

### Specgate rule mapping
- **Primary:** public API bypass / internal capability leakage
- **Specgate-style mapping:** “No raw infrastructure handles in exported module contracts unless explicitly whitelisted.”

### Minimal fixture design
- Module `ingress.ts` creates internal `Server`.
- Exported factory returns `{ server, close }` (bad).
- Rule asserts exported type must not contain Node infra types (`Server`, sockets, db clients, etc.) unless annotated allowlist.
- Positive test: façade-only return (`close`, `address`) passes.

### Confidence
**High** (direct before/after API-shape diff, explicit fix intent in commit message).

---

## 2) Layer inversion at trust boundary: WS origin policy incorrectly applied to HTTP

- **Intro SHA:** `b19749d6a486032e7198f17b9a823d7d90375c2c` (Co-Authored-By: Claude Opus 4.6)
- **Fix SHA:** `567abbd7ed435367e5ab28f881d7f975e5d55dd7` (Codex review finding; Co-Authored-By: Claude Opus 4.6)

### Files touched
- **Intro:**
  - `src/server/index.ts`
  - `src/server/index.test.ts`
  - `src/server/originPolicy.ts`
  - `src/server/originPolicy.test.ts`
  - `src/server/websocket.ts`
  - `src/server/websocket.test.ts`
- **Fix:**
  - `src/server/index.ts`
  - `src/server/index.test.ts`

### Failure pattern + root cause
After hardening, HTTP handler rejected requests when `Origin` header was absent, reusing strict WS-origin semantics for HTTP. That broke legitimate server-to-server/webhook/top-level navigation flows where `Origin` may be missing.

Buggy condition in HTTP layer:
- `if (!isAllowedLocalOrigin(origin)) { ...403... }`

Fixed condition:
- `if (origin && !isAllowedLocalOrigin(origin)) { ...403... }`

### Specgate rule mapping
- **Primary:** layer inversion / boundary policy conflation
- **Specgate-style mapping:** “Protocol-specific trust rules must not be applied across layers without explicit adapter logic.”

### Minimal fixture design
- `wsGuard(origin)` where absent origin is invalid.
- `httpGuard(origin)` where absent origin is acceptable but mismatched present origin is blocked.
- Bad implementation reuses `wsGuard` in HTTP path.
- Rule/fixture expects explicit transport-aware guard split (`HTTP` vs `WS`) and catches shared misuse.

### Confidence
**High** (very clear semantic delta and test expectation change `403 -> 204`).

---

## 3) Auth boundary violation via env-precedence bug (dev flag overriding prod auth path)

- **Intro SHA:** `a30fdca629d0a4427154220c1afd860eb8a7d762` (Co-Authored-By: Claude Opus 4.6)
- **Fix SHA:** `90c76f9a3c0ac0bb59cfe3aae22e2b56a3f1ba30` (Codex review findings; Co-Authored-By: Claude Opus 4.6)

### Files touched
- **Intro (key):**
  - `src/server/index.ts`
  - `src/server/index.test.ts`
  - `src/server/sessionEndpoint.ts`
  - `src/server/sessionEndpoint.test.ts`
  - `src/server/ticketStore.ts`
  - `src/server/ticketStore.test.ts`
  - `src/client/constants/config.ts`
  - `src/client/lib/authenticateAndConnect.ts`
- **Fix (key):**
  - `src/server/index.ts`
  - `src/server/index.test.ts`
  - `src/server/sessionEndpoint.ts`
  - `src/server/sessionEndpoint.test.ts`
  - `src/client/constants/config.ts`
  - `src/client/constants/config.test.ts`

### Failure pattern + root cause
Upgrade auth check gated on `!DEV_ALLOW_UNAUTH && ticketStore`, allowing auth bypass if both `HEARTH_CLIENT_TOKEN` and `HEARTH_DEV_ALLOW_UNAUTH=true` were set. Dev-mode switch crossed into production auth boundary.

Fix changed guard to `if (ticketStore) { ...require ticket... }`, making configured token/ticket auth authoritative regardless of dev flag; also added warning for dual-env misconfiguration.

### Specgate rule mapping
- **Primary:** dependency/trust-boundary violation (configuration layer overriding security boundary)
- **Specgate-style mapping:** “Security-critical path must not depend on non-prod convenience flags when secure credentials are configured.”

### Minimal fixture design
- Config module exposes `CLIENT_TOKEN` and `DEV_ALLOW_UNAUTH`.
- Upgrade gate currently: `if (!DEV_ALLOW_UNAUTH && tokenStore) requireTicket()`.
- Fixture sets both values; expects secure behavior (ticket required).
- Rule flags precedence where debug flag can disable auth when secure credential exists.

### Confidence
**High** (explicitly documented as High severity in fix commit; direct guard rewrite).

---

## Notes for corpus curation
- All three examples have concrete intro/fix SHAs and precise file anchors.
- Examples 1 and 2 are strongest for **Specgate architectural boundary checks**.
- Example 3 is strongest for **policy boundary precedence** (security-critical config layering).
