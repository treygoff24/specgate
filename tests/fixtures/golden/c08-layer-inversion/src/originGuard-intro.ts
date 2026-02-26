// INTRO version: Shared origin guard - breaks HTTP when Origin is absent

const ALLOWED_ORIGINS = ['http://localhost:3000', 'https://app.example.com'];

// Shared guard - used by both HTTP and WS (layer violation)
export function isAllowedLocalOrigin(origin: string | null): boolean {
  if (!origin) return false; // BUG: This breaks HTTP server-to-server!
  return ALLOWED_ORIGINS.includes(origin);
}

export function handleHttpRequest(req: Request): Response {
  const origin = req.headers.get('origin');
  if (!isAllowedLocalOrigin(origin)) {
    return new Response('Forbidden', { status: 403 });
  }
  return new Response('OK', { status: 200 });
}

export function handleWsUpgrade(req: Request): Response {
  const origin = req.headers.get('origin');
  if (!isAllowedLocalOrigin(origin)) {
    return new Response('Forbidden', { status: 403 });
  }
  return new Response('Upgrade Required', { status: 426 });
}
