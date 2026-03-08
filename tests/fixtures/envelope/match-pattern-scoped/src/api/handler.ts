import { boundary } from 'specgate-envelope';

const payload = { ok: true };

export function createUser() {
  boundary.validate('create_user', payload);
}

export function deleteUser() {
  return payload;
}
