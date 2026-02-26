import { persist } from '../domain/facade';

export const handle = (payload: unknown) => persist(payload);
