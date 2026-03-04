// VIOLATION: shared has no allow_imports_from, so importing from web is forbidden
import { helper } from '../../web/src/helper';

export function sneaky(): string {
  return helper();
}
