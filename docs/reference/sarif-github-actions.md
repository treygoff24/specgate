# SARIF Output & GitHub Code Scanning

Specgate can emit SARIF 2.1.0 output for GitHub Code Scanning.

## Usage

```bash
specgate check --format sarif > specgate.sarif
```

## GitHub Actions Integration

```yaml
name: Specgate Policy Check
on: [push, pull_request]

jobs:
  specgate:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Install specgate
        run: cargo install --locked --git https://github.com/treygoff24/specgate --tag vX.Y.Z
      - name: Run specgate
        run: specgate check --format sarif > specgate.sarif
      - name: Upload SARIF
        if: always() && hashFiles('specgate.sarif') != ''
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: specgate.sarif
```

## How it works

- Each policy violation becomes a SARIF `result`
- Rule IDs map to SARIF `reportingDescriptor` entries
- File locations include line and column when available
- Violation fingerprints enable stable baseline tracking across runs

This example preserves normal `specgate check` exit behavior, so policy
violations still fail CI while SARIF upload runs as a follow-on reporting step.
