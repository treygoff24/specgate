# C09: Public API Leakage (Internal Server Object Exposed)

## Provenance
- **Intro SHA:** `89d79b1cdb3434a92d64484146e54691e6b27856`
- **Fix SHA:** `cbd164298efc65168ea25f265f0a4bec64af2ad1`
- **Repo:** `/Users/treygoff/Development/hearth`

## Root Cause
`createWebhookIngress()` returned `{ server, close }`, leaking Node's raw `http.Server` object.

## Status
⚠️ **SEMANTIC PROXY** - `boundary.public_api` controls which files can be imported, not what types are exported.
