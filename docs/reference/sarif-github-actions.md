# SARIF Output & GitHub Code Scanning

specgate can output violations in SARIF 2.1.0 format for integration with GitHub Code Scanning.

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
    steps:
      - uses: actions/checkout@v4
      - name: Install specgate
        run: cargo install specgate
      - name: Run specgate
        run: specgate check --format sarif > specgate.sarif
        continue-on-error: true
      - name: Upload SARIF
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: specgate.sarif
```

## How It Works

- Each policy violation becomes a SARIF `result`
- Rule IDs map to SARIF `reportingDescriptor` entries
- File locations include line and column when available
- Violation fingerprints enable stable baseline tracking across runs
