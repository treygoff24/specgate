"use strict";

const {
  generateResolutionSnapshot,
  runCli,
  wrapperVersion,
} = require("./generate-resolution-snapshot");

module.exports = {
  generateResolutionSnapshot,
  runResolutionSnapshotCli: runCli,
  wrapperVersion,
};
