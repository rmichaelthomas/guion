import { createHash } from 'node:crypto';
import { existsSync, statSync } from 'node:fs';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const MAX_PAGE_BYTES = 400 * 1024;
const ALLOWED_SCRIPT = /^<script type="module" src="(?:\.\.\/)?playground\/loader\.mjs"><\/script>$/;
const REMOTE = /^(?:\/|https?:|\/\/)/i;
// A canonical link names the page's own public address; it is metadata, not a request.
const CANONICAL = /<link rel="canonical" href="https:\/\/rmichaelthomas\.github\.io\/guion\/">/g;
// Structured data for search engines is inert JSON, never executed.
const STRUCTURED_DATA = /^<script type="application\/ld\+json">\{[\s\S]*\}<\/script>$/;

async function walk(dir, prefix = '') {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const relative = path.posix.join(prefix, entry.name);
    if (entry.isDirectory()) found.push(...await walk(path.join(dir, entry.name), relative));
    else found.push(relative);
  }
  return found.sort();
}

const references = (html) => [...html.replace(CANONICAL, '').matchAll(/\b(?:href|src)\s*=\s*"([^"]*)"/g)].map((match) => match[1]);
const isInternalPath = (value) => value !== '' && !value.startsWith('#') && !/^(?:mailto:|data:)/i.test(value) && !REMOTE.test(value);

/**
 * Check the deployed directory against the site's rules.
 * R1 every file is linked (or a declared asset) · R2 anchors resolve · R3 no absolute or remote URLs but the canonical link ·
 * R4 the only scripts are the playground loader and structured data · R5 evidence hashes match · R6 page weight · R7 .nojekyll
 */
export async function checkSite(publicDir) {
  const errors = [];
  const files = await walk(publicDir);
  const pages = files.filter((file) => file.endsWith('.html'));
  const text = new Map();
  for (const file of pages) text.set(file, await readFile(path.join(publicDir, file), 'utf8'));

  if (!files.includes('.nojekyll')) errors.push('R7 missing .nojekyll');

  for (const file of pages) {
    const html = text.get(file);
    const ids = new Set([...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]));
    for (const value of references(html)) {
      if (value.startsWith('#') && value.length > 1 && !ids.has(value.slice(1))) errors.push(`R2 ${file}: ${value} has no matching id`);
      if (!/^(?:mailto:|data:)/i.test(value) && REMOTE.test(value)) errors.push(`R3 ${file}: absolute or remote reference "${value}"`);
    }
    for (const block of html.matchAll(/<style\b[^>]*>([\s\S]*?)<\/style>/g)) {
      for (const match of block[1].matchAll(/url\(\s*['"]?([^'")\s]+)/g)) {
        if (!/^data:/i.test(match[1]) && REMOTE.test(match[1])) errors.push(`R3 ${file}: absolute or remote reference "${match[1]}"`);
      }
      if (/@import/.test(block[1])) errors.push(`R3 ${file}: @import is not allowed`);
    }
    for (const script of html.matchAll(/<script\b[^>]*>[\s\S]*?<\/script>/g)) {
      if (!ALLOWED_SCRIPT.test(script[0]) && !STRUCTURED_DATA.test(script[0])) errors.push(`R4 ${file}: unexpected script ${script[0]}`);
    }
    const bytes = Buffer.byteLength(html);
    if (bytes > MAX_PAGE_BYTES) errors.push(`R6 ${file} is ${Math.round(bytes / 1024)} KB, over the ${MAX_PAGE_BYTES / 1024} KB budget`);
  }

  // R1: everything must be reachable from index.html, except the playground tree the loader fetches by name.
  const reachable = new Set(['.nojekyll']);
  const queue = pages.includes('index.html') ? ['index.html'] : [];
  const seen = new Set(queue);
  while (queue.length > 0) {
    const file = queue.shift();
    reachable.add(file);
    if (!text.has(file)) continue;
    for (const value of references(text.get(file)).filter(isInternalPath)) {
      let target = path.posix.normalize(path.posix.join(path.posix.dirname(file), value.split(/[?#]/)[0]));
      if (target === '.' || value.endsWith('/') || (existsSync(path.join(publicDir, target)) && statSync(path.join(publicDir, target)).isDirectory())) {
        target = path.posix.join(target === '.' ? '' : target, 'index.html');
      }
      if (!files.includes(target)) {
        errors.push(`R1 broken link in ${file}: ${value}`);
        continue;
      }
      reachable.add(target);
      if (target.endsWith('.html') && !seen.has(target)) {
        seen.add(target);
        queue.push(target);
      }
    }
  }
  for (const file of files) {
    if (!reachable.has(file) && !file.startsWith('playground/')) errors.push(`R1 not linked from any page: ${file}`);
  }

  // R5: evidence hashes in the generated index must match the files.
  if (text.has('evidence/index.html')) {
    const listed = new Map();
    for (const row of text.get('evidence/index.html').matchAll(/<tr>([\s\S]*?)<\/tr>/g)) {
      const name = /<a href="([^"]+)">/.exec(row[1])?.[1];
      const hash = /<code>([0-9a-f]{64})<\/code>/.exec(row[1])?.[1];
      if (name && hash) listed.set(name, hash);
    }
    for (const file of files.filter((entry) => entry.startsWith('evidence/') && entry !== 'evidence/index.html')) {
      const name = file.slice('evidence/'.length);
      if (!listed.has(name)) {
        errors.push(`R5 ${file}: not listed with a hash in evidence/index.html`);
        continue;
      }
      const actual = createHash('sha256').update(await readFile(path.join(publicDir, file))).digest('hex');
      if (actual !== listed.get(name)) errors.push(`R5 ${file}: sha256 does not match evidence/index.html`);
    }
  }

  errors.sort();
  return { errors, report: { files: files.length, indexBytes: text.has('index.html') ? Buffer.byteLength(text.get('index.html')) : 0 } };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const target = path.resolve(process.argv[2] ?? 'site/public');
  const { errors, report } = await checkSite(target);
  if (errors.length > 0) {
    console.error(errors.join('\n'));
    console.error(`\n${errors.length} problem(s) in ${target}`);
    process.exit(1);
  }
  console.log(`site check passed: ${report.files} files, index.html ${Math.round(report.indexBytes / 1024)} KB`);
}
