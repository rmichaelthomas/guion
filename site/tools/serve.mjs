import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { SITE_ROOT } from './lib/paths.mjs';

const PREFIX = '/guion/';
const TYPES = {
  '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8', '.json': 'application/json; charset=utf-8', '.wasm': 'application/wasm',
  '.md': 'text/markdown; charset=utf-8', '.gu': 'text/plain; charset=utf-8', '.txt': 'text/plain; charset=utf-8',
};

/** Serve `root` at /guion/ the way GitHub Pages serves this project, so subpath mistakes show up locally. */
export async function startServer({ root, port = 8080 }) {
  const absoluteRoot = path.resolve(root);
  const server = createServer(async (request, response) => {
    try {
      const pathname = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
      if (!pathname.startsWith(PREFIX)) throw Object.assign(new Error('outside prefix'), { code: 'ENOENT' });
      let file = path.join(absoluteRoot, pathname.slice(PREFIX.length));
      if (file !== absoluteRoot && !file.startsWith(absoluteRoot + path.sep)) throw Object.assign(new Error('outside root'), { code: 'ENOENT' });
      if ((await stat(file)).isDirectory()) file = path.join(file, 'index.html');
      const body = await readFile(file);
      response.writeHead(200, { 'content-type': TYPES[path.extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store' });
      response.end(body);
    } catch {
      response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
      response.end('Not found');
    }
  });
  await new Promise((resolve) => server.listen(port, '127.0.0.1', resolve));
  return { server, url: `http://127.0.0.1:${server.address().port}` };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { url } = await startServer({ root: path.join(SITE_ROOT, 'public'), port: Number(process.argv[2] ?? 8080) });
  console.log(`Serving site/public at ${url}${PREFIX}`);
}
