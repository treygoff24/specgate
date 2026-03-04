// Intentional circular dependency for cycle detection testing
import { webValue } from '../../web/src/core.js';
export const cycleProof = webValue + 1;
