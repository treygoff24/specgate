#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const signals = {
  SIGHUP: 1,
  SIGINT: 2,
  SIGQUIT: 3,
  SIGILL: 4,
  SIGTRAP: 5,
  SIGABRT: 6,
  SIGBUS: 7,
  SIGFPE: 8,
  SIGKILL: 9,
  SIGUSR1: 10,
  SIGSEGV: 11,
  SIGUSR2: 12,
  SIGPIPE: 13,
  SIGALRM: 14,
  SIGTERM: 15,
  SIGSTKFLT: 16,
  SIGCHLD: 17,
  SIGCONT: 18,
  SIGSTOP: 19,
  SIGTSTP: 20,
  SIGTTIN: 21,
  SIGTTOU: 22,
  SIGURG: 23,
  SIGXCPU: 24,
  SIGXFSZ: 25,
  SIGVTALRM: 26,
  SIGPROF: 27,
  SIGWINCH: 28,
  SIGIO: 29,
  SIGSYS: 31,
};

function isFile(pathname) {
  try {
    return fs.statSync(pathname).isFile();
  } catch {
    return false;
  }
}

function signalExitCode(signalName) {
  if (process.platform === "win32") {
    return 1;
  }

  const signalCode = signals[signalName];
  return typeof signalCode === "number" ? 128 + signalCode : 128;
}

function binaryName() {
  return process.platform === "win32" ? "specgate.exe" : "specgate";
}

function nativeCandidates() {
  const candidates = [];

  if (process.env.SPECGATE_NATIVE_BIN) {
    const nativeBinPath = path.isAbsolute(process.env.SPECGATE_NATIVE_BIN)
      ? process.env.SPECGATE_NATIVE_BIN
      : path.resolve(__dirname, "..", process.env.SPECGATE_NATIVE_BIN);
    candidates.push(nativeBinPath);
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
    "  specgate resolution-snapshot (or snapshot-resolution) --from <file> --import <specifier> [options]",
    "  specgate <native-specgate-args>",
    "",
    "Native binary lookup order:",
    "  1) SPECGATE_NATIVE_BIN env var",
    "  2) npm/specgate/native/<platform>/<arch>/specgate",
    "  3) npm/specgate/native/<platform>/specgate",
    "",
    "Resolution snapshot help:",
    "  specgate resolution-snapshot --help",
    "  specgate snapshot-resolution --help (alias)",
  ];

  process.stdout.write(`${text.join("\n")}\n`);
}

function runResolutionSnapshot(args) {
  const { runCli } = require("../src/generate-resolution-snapshot");
  return runCli(args);
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

    if (result.signal) {
      const signalCode = signalExitCode(result.signal);
      if (process.platform === "win32") {
        process.stderr.write(`Native specgate binary was killed by signal ${result.signal} (exit ${signalCode})\n`);
      } else {
        const signalNumber = signals[result.signal] ?? 0;
        process.stderr.write(`Native specgate binary was killed by signal ${result.signal} (exit 128+${signalNumber})\n`);
      }
      return signalCode;
    }

    if (result.error) {
      process.stderr.write(`Failed to launch native specgate binary '${path.relative(process.cwd(), candidate)}': ${result.error.message}\n`);
      return 1;
    }

    process.stderr.write(`Native specgate binary exited unexpectedly (no status, signal, or error)\n`);
    return 1;
  }

  const candidates = nativeCandidates();
  const candidateList = candidates.map((candidate) => `  - ${candidate}`).join("\n");
  process.stderr.write([
    "No native specgate binary found.",
    `platform: ${process.platform}`,
    `arch: ${process.arch}`,
    "searched:",
    candidateList,
    "Provide SPECGATE_NATIVE_BIN to the environment to specify where the binary lives.",
    "",
  ].join("\n"));
  return 1;
}

function main(argv) {
  const [subcommand, ...rest] = argv;

  if (!subcommand || subcommand === "--help" || subcommand === "help") {
    printWrapperHelp();
    return 0;
  }

  if (subcommand === "resolution-snapshot" || subcommand === "snapshot-resolution") {
    return runResolutionSnapshot(rest);
  }

  return runNativeSpecgate(argv);
}

process.exitCode = main(process.argv.slice(2));
