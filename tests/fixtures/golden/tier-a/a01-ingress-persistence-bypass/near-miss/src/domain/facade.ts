import { saveRaw } from '../infra/db/client';

export const persist = (payload: unknown) => saveRaw(payload);
