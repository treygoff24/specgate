import { allowOrigin } from './origin_gate';

export const shouldAllow = (origin: string) => allowOrigin(origin);
