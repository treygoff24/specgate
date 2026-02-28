#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { runCli } = require("../src/generate-resolution-snapshot");

function isFile(pathname) {
  try {
    return fs.statSync(pathname).isFile();
  } catch {
    return false;
  }
}

function binaryName() {
  return process.platform === "win32" ? "specgate.exe" : "specgate";
}

function nativeCandidates() {
  const candidates = [];

  if (process.env.SPECGATE_NATIVE_BIN) {
    candidates.push(path.resolve(process.cwd(), process.env.SPECGATE_NATIVE_BIN));
  }

  candidates.push(path.resolve(__dirname, "..", "native", process.platform, process.arch, binaryName()));
  candidates.push(path.resolve(__dirname, "..", "native", process.platform, binaryName()));

  return candidates;
}

function printWrapperHelp() {
  const text = [
    "specgate npm wrapper",
    "",
    "Usage:",
    "  specgate resolution-snapshot --from <file> --import <specifier> [options]",
    "  specgate <native-specgate-args>",
    "",
    "Native binary lookup order:",
    "  1) SPECGATE_NATIVE_BIN env var",
    "  2) npm/specgate/native/<platform>/<arch>/specgate",
    "  3) npm/specgate/native/<platform>/specgate",
    "",
    "Resolution snapshot help:",
    "  specgate resolution-snapshot --help"
  ];

  process.stdout.write(`${text.join("\n")}\n`);
}

function runNativeSpecgate(args) {
  for (const candidate of nativeCandidates()) {
    if (!isFile(candidate)) {
      continue;
    }

    const result = spawnSync(candidate, args, { stdio: "inherit" });
    if (typeof result.status === "number") {
      return result.status;
    }

    if (result.error) {
      process.stderr.write(`Failed to launch native specgate binary '${path.relative(process.cwd(), candidate)}': ${result.error.message}\n`);
      return 1;
    }

    return 1;
  }

  process.stderr.write(
    "No native specgate binary found. Provide SPECGATE_NATIVE_BIN or bundle one under npm/specgate/native/.\n"
  );
  return 1;
}

function main(argv) {
  const [subcommand, ...rest] = argv;

  if (!subcommand || subcommand === "--help" || subcommand === "help") {
    printWrapperHelp();
    return 0;
  }

  if (subcommand === "resolution-snapshot" || subcommand === "snapshot-resolution") {
    return runCli(rest);
  }

  return runNativeSpecgate(argv);
}

process.exitCode = main(process.argv.slice(2));
