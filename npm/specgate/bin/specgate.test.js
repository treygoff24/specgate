"use strict";

const assert = require("node:assert");
const path = require("node:path");
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

function it(name, fn) {
  try {
    fn();
    process.stdout.write(`✓ ${name}\n`);
  } catch (error) {
    process.stdout.write(`✗ ${name}\n`);
    process.stderr.write(`${error.stack}\n`);
    process.exitCode = 1;
  }
}

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
