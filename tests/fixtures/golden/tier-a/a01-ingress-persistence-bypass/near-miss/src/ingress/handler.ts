import type { PersistedRecord } from '../infra/db/client';
import { persist } from '../domain/facade';

export const handle = (payload: PersistedRecord) => persist(payload.payload);
