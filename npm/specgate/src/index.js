"use strict";

const {
  generateResolutionSnapshot,
  runCli,
} = require("./generate-resolution-snapshot");

module.exports = {
  generateResolutionSnapshot,
  runResolutionSnapshotCli: runCli,
};
