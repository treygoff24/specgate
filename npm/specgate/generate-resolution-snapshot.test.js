"use strict";

const {
  classifyResolution,
  toProjectPath,
  parseArgs,
  isBuiltinImport,
  generateResolutionSnapshot,
} = require("./src/generate-resolution-snapshot");

const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

function describe(name, fn) {
  console.log(`\n${name}`);
  fn();
}

function it(name, fn) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    console.log(`  ✗ ${name}`);
    console.log(`    ${error.message}`);
    process.exitCode = 1;
  }
}

function assertEqual(actual, expected, message) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `${message || "assertion failed"}\n    expected: ${JSON.stringify(expected)}\n    actual: ${JSON.stringify(actual)}`
    );
  }
}

function assertTrue(value, message) {
  if (!value) {
    throw new Error(message || "expected true");
  }
}

function assertFalse(value, message) {
  if (value) {
    throw new Error(message || "expected false");
  }
}

describe("isBuiltinImport", () => {
  it("returns true for node:fs", () => {
    assertTrue(isBuiltinImport("node:fs"));
  });

  it("returns true for fs", () => {
    assertTrue(isBuiltinImport("fs"));
  });

  it("returns true for node:path", () => {
    assertTrue(isBuiltinImport("node:path"));
  });

  it("returns true for path", () => {
    assertTrue(isBuiltinImport("path"));
  });

  it("returns false for relative imports", () => {
    assertFalse(isBuiltinImport("./utils"));
  });

  it("returns false for package imports", () => {
    assertFalse(isBuiltinImport("lodash"));
  });

  it("returns false for scoped packages", () => {
    assertFalse(isBuiltinImport("@org/package"));
  });
});

describe("classifyResolution", () => {
  it("classifies unresolvable imports", () => {
    const result = classifyResolution("./nonexistent", null);
    assertEqual(result.resultKind, "unresolvable");
    assertEqual(result.resolvedTo, null);
    assertEqual(result.packageName, null);
  });

  it("classifies builtin imports as third_party when no resolved module", () => {
    const result = classifyResolution("fs", null);
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.resolvedTo, null);
    assertEqual(result.packageName, "fs");
  });

  it("classifies node: prefixed builtin as third_party when no resolved module", () => {
    const result = classifyResolution("node:path", null);
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.resolvedTo, null);
    assertEqual(result.packageName, "path");
  });

  it("classifies external library imports as third_party", () => {
    const result = classifyResolution(
      "lodash",
      {
        resolvedFileName: "/node_modules/lodash/index.js",
        isExternalLibraryImport: true,
      }
    );
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.packageName, "lodash");
  });

  it("classifies first_party imports", () => {
    const result = classifyResolution(
      "./utils",
      {
        resolvedFileName: "/src/utils.ts",
        isExternalLibraryImport: false,
      }
    );
    assertEqual(result.resultKind, "first_party");
    assertEqual(result.resolvedTo, "/src/utils.ts");
    assertEqual(result.packageName, null);
  });

  it("classifies node_modules path as third_party even without isExternalLibraryImport", () => {
    const result = classifyResolution(
      "lodash",
      {
        resolvedFileName: "/project/node_modules/lodash/index.js",
        isExternalLibraryImport: false,
      }
    );
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.packageName, "lodash");
  });
});

describe("toProjectPath", () => {
  it("returns relative path for files inside project", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-"));
    try {
      const result = toProjectPath(tmpDir, path.join(tmpDir, "src", "utils.ts"));
      assertEqual(result, "src/utils.ts");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns . for project root", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-"));
    try {
      const result = toProjectPath(tmpDir, tmpDir);
      assertEqual(result, ".");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns absolute path for files outside project", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-"));
    const outsideFile = path.join(os.tmpdir(), "outside.ts");
    try {
      fs.writeFileSync(outsideFile, "");
      const result = toProjectPath(tmpDir, outsideFile);
      assertTrue(result.startsWith("/"));
      assertTrue(result.includes("outside.ts"));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
      try {
        fs.unlinkSync(outsideFile);
      } catch {
        // ignore
      }
    }
  });

  it("normalizes path separators to forward slashes", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-"));
    try {
      const result = toProjectPath(tmpDir, path.join(tmpDir, "src", "utils.ts"));
      assertTrue(!result.includes("\\"), `path should not contain backslashes: ${result}`);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

describe("parseArgs", () => {
  it("parses required arguments", () => {
    const args = parseArgs(["--from", "src/main.ts", "--import", "lodash"]);
    assertEqual(args.from, "src/main.ts");
    assertEqual(args.importSpecifier, "lodash");
  });

  it("parses optional arguments", () => {
    const args = parseArgs([
      "--from",
      "src/main.ts",
      "--import",
      "lodash",
      "--project-root",
      "/project",
      "--tsconfig",
      "tsconfig.json",
      "--out",
      "output.json",
      "--pretty",
    ]);
    assertEqual(args.from, "src/main.ts");
    assertEqual(args.importSpecifier, "lodash");
    assertEqual(args.projectRoot, "/project");
    assertEqual(args.tsconfig, "tsconfig.json");
    assertEqual(args.output, "output.json");
    assertTrue(args.pretty);
  });

  it("parses --import-specifier alias", () => {
    const args = parseArgs(["--from", "src/main.ts", "--import-specifier", "lodash"]);
    assertEqual(args.importSpecifier, "lodash");
  });

  it("parses --output alias", () => {
    const args = parseArgs(["--from", "src/main.ts", "--import", "lodash", "--output", "output.json"]);
    assertEqual(args.output, "output.json");
  });

  it("throws on missing value", () => {
    let error;
    try {
      parseArgs(["--from"]);
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("missing value"));
  });

  it("throws on flag as value", () => {
    let error;
    try {
      parseArgs(["--from", "--import", "lodash"]);
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("requires a value but got flag"));
  });

  it("throws on unknown argument", () => {
    let error;
    try {
      parseArgs(["--from", "src/main.ts", "--import", "lodash", "--unknown", "value"]);
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("unknown argument"));
  });

  it("parses --help flag", () => {
    const args = parseArgs(["--help"]);
    assertTrue(args.help);
  });

  it("parses -h flag", () => {
    const args = parseArgs(["-h"]);
    assertTrue(args.help);
  });
});

describe("generateResolutionSnapshot", () => {
  it("throws on missing --from", () => {
    let error;
    try {
      generateResolutionSnapshot({ importSpecifier: "lodash" });
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("missing required --from"));
  });

  it("throws on missing --import", () => {
    let error;
    try {
      generateResolutionSnapshot({ from: "src/main.ts" });
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("missing required --import"));
  });

  it("throws on non-existent --from file", () => {
    let error;
    try {
      generateResolutionSnapshot({ from: "nonexistent.ts", importSpecifier: "lodash" });
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("--from file not found"));
  });

  it("generates snapshot for builtin import", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-"));
    const testFile = path.join(tmpDir, "test.ts");
    const tsconfigPath = path.join(tmpDir, "tsconfig.json");

    try {
      fs.writeFileSync(testFile, 'import "fs";');
      fs.writeFileSync(
        tsconfigPath,
        JSON.stringify({
          compilerOptions: {
            module: "NodeNext",
            moduleResolution: "NodeNext",
          },
        })
      );

      const snapshot = generateResolutionSnapshot({
        from: testFile,
        importSpecifier: "fs",
        projectRoot: tmpDir,
        tsconfig: tsconfigPath,
      });

      assertEqual(snapshot.schema_version, "1");
      assertEqual(snapshot.snapshot_kind, "doctor_compare_tsc_resolution_focus");
      assertEqual(snapshot.focus.from, "test.ts");
      assertEqual(snapshot.focus.import_specifier, "fs");
      assertTrue(snapshot.resolutions.length > 0);
      assertEqual(snapshot.resolutions[0].import, "fs");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("generates snapshot for relative import", () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-"));
    const testFile = path.join(tmpDir, "src", "test.ts");
    const targetFile = path.join(tmpDir, "src", "utils.ts");
    const tsconfigPath = path.join(tmpDir, "tsconfig.json");

    try {
      fs.mkdirSync(path.dirname(testFile), { recursive: true });
      fs.writeFileSync(testFile, 'import "./utils";');
      fs.writeFileSync(targetFile, "export const x = 1;");
      fs.writeFileSync(
        tsconfigPath,
        JSON.stringify({
          compilerOptions: {
            module: "NodeNext",
            moduleResolution: "NodeNext",
          },
        })
      );

      const snapshot = generateResolutionSnapshot({
        from: testFile,
        importSpecifier: "./utils",
        projectRoot: tmpDir,
        tsconfig: tsconfigPath,
      });

      assertEqual(snapshot.schema_version, "1");
      assertEqual(snapshot.focus.from, "src/test.ts");
      assertEqual(snapshot.focus.import_specifier, "./utils");
      assertEqual(snapshot.resolutions[0].result_kind, "first_party");
      assertEqual(snapshot.resolutions[0].resolved_to, "src/utils.ts");
      assertTrue(snapshot.edges.length > 0);
      assertEqual(snapshot.edges[0].from, "src/test.ts");
      assertEqual(snapshot.edges[0].to, "src/utils.ts");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

console.log("\n");
