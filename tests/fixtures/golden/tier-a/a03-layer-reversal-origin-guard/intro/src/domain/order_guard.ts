import { allowOrigin } from '../ingress/http/origin_gate';

export const shouldAllow = (origin: string) => allowOrigin(origin);
