// INTRO version: Leaks internal Server object

import { createServer, Server } from 'http';

export function createWebhookIngress(options: { port: number; path: string }): {
  server: Server;  // LEAKED!
  close: () => Promise<void>;
} {
  const server = createServer((req, res) => {
    res.writeHead(200);
    res.end('OK');
  });

  return {
    server,  // LEAKED! Caller can do anything with this
    close: () => new Promise((resolve) => server.close(resolve))
  };
}
