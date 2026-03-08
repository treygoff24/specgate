import { query } from '../database/connection';

export async function getUserFromHandler(id: string) {
  return query(`select * from users where id = '${id}'`);
}
