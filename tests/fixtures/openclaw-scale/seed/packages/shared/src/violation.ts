// shared should not import from web — intentional boundary violation
import { webValue } from '../../web/src/core.js';
export const leaked = webValue;
