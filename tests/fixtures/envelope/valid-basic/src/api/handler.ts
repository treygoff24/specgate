import { boundary } from 'specgate-envelope';

export async function createUser(req: any) {
  boundary.validate('create_user', req.body);
}
