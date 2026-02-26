import { saveRaw, type PersistedRecord } from '../infra/db/client';

export const persist = (payload: unknown): PersistedRecord => saveRaw(payload);
