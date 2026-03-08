export function maybeLog() {
  if (process.env.DEBUG) {
    const d = require('../debug/logger');
    d.log('debug on');
  }
}
