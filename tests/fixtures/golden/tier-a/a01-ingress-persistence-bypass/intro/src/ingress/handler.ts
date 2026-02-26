import { saveRaw } from '../infra/db/client';

export const handle = (payload: unknown) => saveRaw(payload);
