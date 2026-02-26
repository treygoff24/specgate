import { allowOrigin } from '../../domain/origin_gate';

export const ingressShouldAllow = (origin: string) => allowOrigin(origin);
