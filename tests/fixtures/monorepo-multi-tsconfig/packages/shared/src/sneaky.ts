// VIOLATION: shared module must not import from web
import { helper } from '@web/helper';

export function sneaky(): string {
  return helper();
}
