import { boundary } from 'specgate-envelope';

export async function createUser(req: any) {
  boundary.validate('wrong_id', req.body);
}
