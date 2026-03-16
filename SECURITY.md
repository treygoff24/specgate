# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability in Specgate, please report it responsibly.

**Do not file a public GitHub issue for security vulnerabilities.**

Instead, use [GitHub's private vulnerability reporting](https://github.com/treygoff24/specgate/security/advisories/new) to submit your report. This ensures the issue is triaged privately before any public disclosure.

## Scope

Specgate is a static analysis CLI tool. It reads source files and spec files from disk, parses them, and produces diagnostic output. It does not run user code, open network connections, or execute shell commands (except via the explicit `--allow-shell` flag on `doctor compare --tsc-command`).

Security-relevant areas include:

- **`--allow-shell` flag**: Executes a user-provided command via `sh -lc`. Only used with explicit opt-in. Documented with a security warning in the spec language reference.
- **Path traversal**: Spec files reference file paths. Specgate resolves these relative to the project root. Path traversal outside the project root should not be possible but is in scope for reports.
- **Dependency supply chain**: Specgate depends on `oxc`, `oxc-resolver`, `serde`, `clap`, and other Rust crates. Vulnerabilities in dependencies are in scope.

## Response

We aim to acknowledge reports within 48 hours and provide an initial assessment within 7 days. Fixes for confirmed vulnerabilities will be released as patch versions.

## Supported versions

Security fixes are applied to the latest release only. We do not backport fixes to older release branches.

| Version | Supported |
|---------|-----------|
| 0.3.x   | ✅ Current |
| < 0.3.0 | ❌ Upgrade recommended |
