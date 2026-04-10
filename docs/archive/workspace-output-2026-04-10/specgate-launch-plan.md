# Specgate Open Source Launch — Autonomous Build Plan

## Checklist

- [x] **1. CONTRIBUTING.md** — Write contributor guide: how to file issues, submit PRs, code style (rustfmt + clippy), test requirements (cargo test must pass), branch naming, commit message format. Reference AGENTS.md for agent contributors.
- [x] **2. SECURITY.md** — Vulnerability reporting process. GitHub private disclosure, response SLA, scope.
- [x] **3. CODE_OF_CONDUCT.md** — Standard Contributor Covenant v2.1.
- [x] **4. README hero section** — Add 2-sentence "what and why" at very top. Add quick install via cargo install or npx. Move detailed tarball+checksum install to a subsection. Keep existing content but reorder for external audience.
- [x] **5. GitHub repo metadata** — Set description, homepage URL, topic tags via gh CLI.
- [x] **6. npm publish readiness** — Verify npm/specgate is publishable. Check if it needs org scope or is fine as `specgate`. Confirm bin entry works. Note: actual publish requires npm credentials from Trey.
- [x] **7. Convert TODOs to GitHub issues** — File issues for the two source TODOs (glob overlap detection, legacy trace parser removal).
- [x] **8. SPECGATE_FOR_AGENTS.md version bump** — Update from v0.3.0 to v0.3.1.
- [x] **9. Code review** — Spawn Nous agent to review all changes for quality.
- [x] **10. Final verification** — cargo test, cargo clippy, cargo fmt --check all pass. All new files committed and pushed.
