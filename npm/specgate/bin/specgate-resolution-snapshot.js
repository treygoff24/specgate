#!/usr/bin/env node
"use strict";

const { runCli } = require("../src/generate-resolution-snapshot");

process.exitCode = runCli(process.argv.slice(2));
