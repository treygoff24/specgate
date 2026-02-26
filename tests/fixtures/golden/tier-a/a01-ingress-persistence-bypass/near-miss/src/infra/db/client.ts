export type PersistedRecord = { id: string; payload: unknown };
export const saveRaw = (payload: unknown): PersistedRecord => ({
  id: 'near-miss',
  payload,
});
