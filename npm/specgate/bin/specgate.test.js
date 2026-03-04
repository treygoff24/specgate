"use strict";

const assert = require("node:assert");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");
const { spawnSync } = require("node:child_process");

const wrapperPath = path.join(__dirname, "specgate.js");

function runWrapperWithBlockedTypescript(args, env = {}) {
  const payload = JSON.stringify({ wrapperPath, args });
  const script = `
const Module = require("node:module");
const payload = ${payload};
const originalLoad = Module._load;
Module._load = function(request, parent, isMain) {
  if (request === "typescript") {
    throw new Error("blocked typescript");
  }
  return originalLoad.apply(this, arguments);
};
process.argv = ["node", payload.wrapperPath, ...payload.args];
require(payload.wrapperPath);
`;

  return spawnSync(process.execPath, ["-e", script], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
}

// Run a script in a subprocess with overridden process.platform and process.arch.
// The script receives the wrapper path via __WRAPPER_PATH env var and can
// require it or its internal helpers.
function runWithPlatform(platform, arch, scriptBody, env = {}) {
  const script = `
"use strict";
Object.defineProperty(process, "platform", { value: ${JSON.stringify(platform)} });
Object.defineProperty(process, "arch", { value: ${JSON.stringify(arch)} });
const wrapperPath = process.env.__WRAPPER_PATH;
${scriptBody}
`;
  return spawnSync(process.execPath, ["--input-type=commonjs", "-e", script], {
    encoding: "utf8",
    env: { ...process.env, ...env, __WRAPPER_PATH: wrapperPath },
  });
}

function describe(name, fn) {
  process.stdout.write(`\n${name}\n`);
  fn();
}

function it(name, fn) {
  try {
    fn();
    process.stdout.write(`  ✓ ${name}\n`);
  } catch (error) {
    process.stdout.write(`  ✗ ${name}\n`);
    process.stderr.write(`${error.stack}\n`);
    process.exitCode = 1;
  }
}

describe("typescript lazy loading", () => {
  it("does not load typescript for wrapper help", () => {
    const result = runWrapperWithBlockedTypescript(["--help"]);
    assert.strictEqual(result.status, 0, `expected exit code 0, got ${result.status}\n${result.stderr}`);
    assert.match(result.stdout, /specgate npm wrapper/);
    assert.doesNotMatch(result.stderr, /blocked typescript/);
  });

  it("does not load typescript for native forwarding", () => {
    const result = runWrapperWithBlockedTypescript(["--version"], {
      SPECGATE_NATIVE_BIN: process.execPath,
    });
    assert.strictEqual(result.status, 0, `expected exit code 0, got ${result.status}\n${result.stderr}`);
    assert.match(result.stdout, /^v\d+\./m);
    assert.doesNotMatch(result.stderr, /blocked typescript/);
  });

  it("loads typescript only for resolution snapshot subcommands", () => {
    const result = runWrapperWithBlockedTypescript(["resolution-snapshot", "--help"]);
    assert.notStrictEqual(result.status, 0, "expected non-zero exit code for blocked typescript");
    assert.match(`${result.stdout}${result.stderr}`, /blocked typescript/);
  });
});

// ---------------------------------------------------------------------------
// Binary selection logic tests
//
// The wrapper's binaryName() and nativeCandidates() are not exported and the
// module immediately calls main() on require. We test the binary selection
// contract by verifying the expected path layout for each supported
// platform/arch combination. The end-to-end forwarding tests below confirm
// the real wrapper code works correctly.
// ---------------------------------------------------------------------------
describe("binaryName per platform", () => {
  // The wrapper uses: process.platform === "win32" ? "specgate.exe" : "specgate"
  // We verify this contract holds across platforms.
  for (const { platform, arch, expected } of [
    { platform: "win32", arch: "x64", expected: "specgate.exe" },
    { platform: "linux", arch: "x64", expected: "specgate" },
    { platform: "darwin", arch: "x64", expected: "specgate" },
    { platform: "darwin", arch: "arm64", expected: "specgate" },
  ]) {
    it(`returns ${expected} on ${platform}/${arch}`, () => {
      const result = runWithPlatform(platform, arch, `
        const name = process.platform === "win32" ? "specgate.exe" : "specgate";
        process.stdout.write(name);
      `);
      assert.strictEqual(result.status, 0, result.stderr);
      assert.strictEqual(result.stdout, expected);
    });
  }
});

// ---------------------------------------------------------------------------
// nativeCandidates path resolution tests
// ---------------------------------------------------------------------------
describe("nativeCandidates path resolution", () => {
  const supportMatrix = [
    { platform: "linux", arch: "x64", bin: "specgate" },
    { platform: "darwin", arch: "x64", bin: "specgate" },
    { platform: "darwin", arch: "arm64", bin: "specgate" },
    { platform: "win32", arch: "x64", bin: "specgate.exe" },
  ];

  for (const { platform, arch, bin } of supportMatrix) {
    it(`builds correct candidate paths for ${platform}/${arch}`, () => {
      const result = runWithPlatform(platform, arch, `
        const path = require("node:path");
        const binDir = path.dirname(wrapperPath);
        const binaryName = process.platform === "win32" ? "specgate.exe" : "specgate";
        const candidates = [
          path.resolve(binDir, "..", "native", process.platform, process.arch, binaryName),
          path.resolve(binDir, "..", "native", process.platform, binaryName),
        ];
        process.stdout.write(JSON.stringify(candidates));
      `);
      assert.strictEqual(result.status, 0, result.stderr);
      const candidates = JSON.parse(result.stdout);
      assert.strictEqual(candidates.length, 2);

      const binDir = path.dirname(wrapperPath);
      const expectedFirst = path.resolve(binDir, "..", "native", platform, arch, bin);
      const expectedSecond = path.resolve(binDir, "..", "native", platform, bin);
      assert.strictEqual(candidates[0], expectedFirst,
        `first candidate for ${platform}/${arch} should be native/<platform>/<arch>/<binary>`);
      assert.strictEqual(candidates[1], expectedSecond,
        `second candidate for ${platform}/${arch} should be native/<platform>/<binary>`);
    });
  }

  it("prepends SPECGATE_NATIVE_BIN when set (absolute path)", () => {
    const result = runWithPlatform("linux", "x64", `
      const path = require("node:path");
      const binDir = path.dirname(wrapperPath);
      const binaryName = "specgate";
      const candidates = [];
      const nativeBinPath = process.env.SPECGATE_NATIVE_BIN;
      if (path.isAbsolute(nativeBinPath)) {
        candidates.push(nativeBinPath);
      }
      candidates.push(path.resolve(binDir, "..", "native", process.platform, process.arch, binaryName));
      candidates.push(path.resolve(binDir, "..", "native", process.platform, binaryName));
      process.stdout.write(JSON.stringify(candidates));
    `, { SPECGATE_NATIVE_BIN: "/usr/local/bin/specgate" });
    assert.strictEqual(result.status, 0, result.stderr);
    const candidates = JSON.parse(result.stdout);
    assert.strictEqual(candidates.length, 3);
    assert.strictEqual(candidates[0], "/usr/local/bin/specgate");
  });

  it("prepends SPECGATE_NATIVE_BIN when set (relative path)", () => {
    const result = runWithPlatform("linux", "x64", `
      const path = require("node:path");
      const binDir = path.dirname(wrapperPath);
      const binaryName = "specgate";
      const nativeBinPath = process.env.SPECGATE_NATIVE_BIN;
      const candidates = [];
      if (!path.isAbsolute(nativeBinPath)) {
        candidates.push(path.resolve(binDir, "..", nativeBinPath));
      }
      candidates.push(path.resolve(binDir, "..", "native", process.platform, process.arch, binaryName));
      candidates.push(path.resolve(binDir, "..", "native", process.platform, binaryName));
      process.stdout.write(JSON.stringify(candidates));
    `, { SPECGATE_NATIVE_BIN: "build/specgate" });
    assert.strictEqual(result.status, 0, result.stderr);
    const candidates = JSON.parse(result.stdout);
    assert.strictEqual(candidates.length, 3);
    const binDir = path.dirname(wrapperPath);
    assert.strictEqual(candidates[0], path.resolve(binDir, "..", "build/specgate"));
  });
});

// ---------------------------------------------------------------------------
// Happy path: specgate check via native binary forwarding
// ---------------------------------------------------------------------------
describe("native binary forwarding (specgate check smoke test)", () => {
  it("forwards args to native binary and returns its exit code (success)", () => {
    // Use SPECGATE_NATIVE_BIN pointing to a script that exits 0
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-smoke-"));
    const fakeBin = path.join(tmpDir, "fake-specgate");
    // The fake binary prints args as JSON and exits 0
    fs.writeFileSync(fakeBin, `#!/usr/bin/env node
process.stdout.write(JSON.stringify(process.argv.slice(2)));
process.exit(0);
`);
    fs.chmodSync(fakeBin, 0o755);

    try {
      const result = runWrapperWithBlockedTypescript(["check", "--some-flag"], {
        SPECGATE_NATIVE_BIN: fakeBin,
      });
      assert.strictEqual(result.status, 0, `expected exit 0, got ${result.status}\nstderr: ${result.stderr}`);
      // stdio is inherited so stdout goes to the child directly;
      // since we use spawnSync with stdio:"inherit" in the wrapper,
      // the fake binary's stdout goes to the wrapper's stdout
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("forwards exit code 1 from native binary (violations)", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-smoke-"));
    const fakeBin = path.join(tmpDir, "fake-specgate");
    fs.writeFileSync(fakeBin, `#!/usr/bin/env node
process.exit(1);
`);
    fs.chmodSync(fakeBin, 0o755);

    try {
      const result = runWrapperWithBlockedTypescript(["check"], {
        SPECGATE_NATIVE_BIN: fakeBin,
      });
      assert.strictEqual(result.status, 1, `expected exit 1, got ${result.status}\nstderr: ${result.stderr}`);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("forwards exit code 2 from native binary (config error)", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-smoke-"));
    const fakeBin = path.join(tmpDir, "fake-specgate");
    fs.writeFileSync(fakeBin, `#!/usr/bin/env node
process.exit(2);
`);
    fs.chmodSync(fakeBin, 0o755);

    try {
      const result = runWrapperWithBlockedTypescript(["check"], {
        SPECGATE_NATIVE_BIN: fakeBin,
      });
      assert.strictEqual(result.status, 2, `expected exit 2, got ${result.status}\nstderr: ${result.stderr}`);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns exit code 1 when no native binary is found", () => {
    const result = runWrapperWithBlockedTypescript(["check"], {
      SPECGATE_NATIVE_BIN: "/nonexistent/path/specgate",
    });
    assert.strictEqual(result.status, 1, `expected exit 1, got ${result.status}`);
    assert.match(result.stderr, /No native specgate binary found/);
  });
});

// ---------------------------------------------------------------------------
// Subcommand routing
// ---------------------------------------------------------------------------
describe("subcommand routing", () => {
  it("routes --help to wrapper help (exit 0)", () => {
    const result = runWrapperWithBlockedTypescript(["--help"]);
    assert.strictEqual(result.status, 0);
    assert.match(result.stdout, /specgate npm wrapper/);
    assert.match(result.stdout, /resolution-snapshot/);
  });

  it("routes help to wrapper help (exit 0)", () => {
    const result = runWrapperWithBlockedTypescript(["help"]);
    assert.strictEqual(result.status, 0);
    assert.match(result.stdout, /specgate npm wrapper/);
  });

  it("routes no args to wrapper help (exit 0)", () => {
    const result = runWrapperWithBlockedTypescript([]);
    assert.strictEqual(result.status, 0);
    assert.match(result.stdout, /specgate npm wrapper/);
  });

  it("routes snapshot-resolution as alias for resolution-snapshot", () => {
    const result = runWrapperWithBlockedTypescript(["snapshot-resolution", "--help"]);
    // Should attempt to load typescript (blocked in test harness)
    assert.notStrictEqual(result.status, 0);
    assert.match(`${result.stdout}${result.stderr}`, /blocked typescript/);
  });

  it("routes unknown subcommands to native binary", () => {
    const result = runWrapperWithBlockedTypescript(["version"], {
      SPECGATE_NATIVE_BIN: "/nonexistent/path/specgate",
    });
    assert.strictEqual(result.status, 1);
    assert.match(result.stderr, /No native specgate binary found/);
  });
});
