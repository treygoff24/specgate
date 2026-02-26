// FIX version: Separate guards for HTTP and WS

const ALLOWED_ORIGINS = ['http://localhost:3000', 'https://app.example.com'];

// WS guard - strict
export function isAllowedWsOrigin(origin: string | null): boolean {
  if (!origin) return false;
  return ALLOWED_ORIGINS.includes(origin);
}

// HTTP guard - lenient on absent origin
export function isAllowedHttpOrigin(origin: string | null): boolean {
  if (!origin) return true; // Allow server-to-server
  return ALLOWED_ORIGINS.includes(origin);
}

export function handleHttpRequest(req: Request): Response {
  const origin = req.headers.get('origin');
  if (!isAllowedHttpOrigin(origin)) {
    return new Response('Forbidden', { status: 403 });
  }
  return new Response('OK', { status: 200 });
}

export function handleWsUpgrade(req: Request): Response {
  const origin = req.headers.get('origin');
  if (!isAllowedWsOrigin(origin)) {
    return new Response('Forbidden', { status: 403 });
  }
  return new Response('Upgrade Required', { status: 426 });
}
