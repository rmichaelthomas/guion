import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { startServer } from '../tools/serve.mjs';

const root = await mkdtemp(path.join(os.tmpdir(), 'serve-'));
await mkdir(path.join(root, 'playground'), { recursive: true });
await writeFile(path.join(root, 'index.html'), '<!doctype html><title>home</title>');
await writeFile(path.join(root, 'playground', 'x.wasm'), Buffer.from([0, 0x61, 0x73, 0x6d]));
await writeFile(path.join(root, 'playground', 'loader.mjs'), 'export {}');
const { server, url } = await startServer({ root, port: 0 });
test.after(() => server.close());

test('serves the site under /guion/ with the right content types', async () => {
  const home = await fetch(`${url}/guion/`);
  assert.equal(home.status, 200);
  assert.match(home.headers.get('content-type'), /text\/html/);
  assert.equal((await fetch(`${url}/guion/playground/x.wasm`)).headers.get('content-type'), 'application/wasm');
  assert.match((await fetch(`${url}/guion/playground/loader.mjs`)).headers.get('content-type'), /javascript/);
});

test('refuses anything outside /guion/ and any path that climbs out of the root', async () => {
  assert.equal((await fetch(`${url}/`)).status, 404);
  assert.equal((await fetch(`${url}/index.html`)).status, 404);
  assert.equal((await fetch(`${url}/guion/%2e%2e/%2e%2e/etc/hosts`)).status, 404);
  assert.equal((await fetch(`${url}/guion/missing.html`)).status, 404);
});
