// FIX version: Returns only façade methods

import { createServer, Server } from 'http';

export function createWebhookIngress(options: { port: number; path: string }): {
  close: () => Promise<void>;
  address: () => ReturnType<Server['address']>;
} {
  const server = createServer((req, res) => {
    res.writeHead(200);
    res.end('OK');
  });

  server.listen(options.port);

  // FIXED: Only return façade methods
  return {
    close: () => new Promise((resolve, reject) => {
      server.close((err) => err ? reject(err) : resolve());
    }),
    address: () => server.address()
  };
}
