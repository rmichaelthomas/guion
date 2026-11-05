import { existsSync } from 'node:fs';
import { cp, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { REPO_ROOT, SITE_ROOT } from './lib/paths.mjs';
import { loadClaims, renderCaseFiles, renderClaimCard } from './claims.mjs';
import { publishEvidence } from './evidence.mjs';
import { highlightGuion } from './highlight.mjs';
import { renderRatioChart } from './ratios.mjs';
import { renderAgentsJson, renderLlmsTxt, pythonGaps } from './agents-surface.mjs';

// Vendored AllHere family CSS goes before `components` so the site's own rules can override it.
const STYLE_BEFORE_VENDOR = ['tokens', 'base', 'layout', 'plain', 'developer'];
const STYLE_AFTER_VENDOR = ['components', 'print'];
const escapeHtml = (text) => String(text).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
const SITE_URL = 'https://rmichaelthomas.github.io/guion/';
const BRAND_FILES = ['mark.svg', 'mark-180.png', 'social-card.png'];
const TOKEN = /\{\{([a-z][a-z-]*)(?::([^{}]+))?\}\}/g;

async function walk(dir, prefix = '') {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const relative = path.posix.join(prefix, entry.name);
    if (entry.isDirectory()) found.push(...await walk(path.join(dir, entry.name), relative));
    else found.push(relative);
  }
  return found.sort();
}

const kb = (bytes) => `${Math.max(10, Math.ceil(bytes / 10240) * 10)} KB`;

function countRemoteRequests(html) {
  const patterns = [/(?<!rel="canonical" )(?:href|src)="(?:https?:)?\/\//g, /url\(\s*['"]?https?:/g, /@import/g];
  return patterns.reduce((total, pattern) => total + (html.match(pattern) ?? []).length, 0);
}

async function readStyles(srcDir, vendorDir) {
  const parts = [];
  const addOwn = async (names) => {
    for (const name of names) {
      const file = path.join(srcDir, 'styles', `${name}.css`);
      if (existsSync(file)) parts.push(await readFile(file, 'utf8'));
    }
  };
  await addOwn(STYLE_BEFORE_VENDOR);
  if (existsSync(vendorDir)) {
    const families = (await readdir(vendorDir)).filter((name) => /^family-.*\.css$/.test(name)).sort();
    for (const name of families) parts.push(`@layer components {\n${await readFile(path.join(vendorDir, name), 'utf8')}}\n`);
  }
  await addOwn(STYLE_AFTER_VENDOR);
  return parts.join('\n');
}

/**
 * Assemble the deployed site from src/ and vendor/ into outDir:
 * index.html (Plain English), developer/index.html (Developer pre-selected), the playground
 * loader and samples, and the evidence files and index.
 */
export async function buildSite({ srcDir, vendorDir, outDir, repoRoot = REPO_ROOT, wasmDir = path.join(outDir, 'playground') }) {
  const shell = await readFile(path.join(srcDir, 'shell.html'), 'utf8');
  const css = await readStyles(srcDir, vendorDir);
  const claims = existsSync(path.join(srcDir, 'claims.json')) ? loadClaims(srcDir) : [];
  const claimById = new Map(claims.map((claim) => [claim.id, claim]));
  const loaderFile = path.join(srcDir, 'playground', 'loader.mjs');
  const jsBytes = existsSync(loaderFile) ? Buffer.byteLength(await readFile(loaderFile, 'utf8')) : 0;
  const wasmBytes = existsSync(wasmDir)
    ? (await Promise.all((await readdir(wasmDir)).filter((name) => name.endsWith('.wasm')).map(async (name) => (await readFile(path.join(wasmDir, name))).byteLength))).reduce((a, b) => a + b, 0)
    : 0;
  const cliFile = path.join(srcDir, 'content', 'cli.json');
  const cli = existsSync(cliFile) ? JSON.parse(await readFile(cliFile, 'utf8')) : null;
  const samplesFile = path.join(srcDir, 'playground', 'samples', 'samples.json');
  const sampleList = existsSync(samplesFile) ? JSON.parse(await readFile(samplesFile, 'utf8')) : [];
  let playground = null;
  const samples = new Map();
  /** Compile and run a sample through the real Wasm modules, so the page shows what they actually do. */
  async function sampleFacts(id) {
    if (!samples.has(id)) {
      if (!playground) {
        const loader = await import(pathToFileURL(loaderFile).href);
        playground = { loader, modules: await loader.loadModules(await readFile(path.join(wasmDir, 'guion_compiler.wasm')), await readFile(path.join(wasmDir, 'guion_vm.wasm'))) };
      }
      const source = await readFile(path.join(srcDir, 'playground', 'samples', `${id}.gu`), 'utf8');
      const compiled = playground.loader.compile(playground.modules, source);
      const result = compiled.ok ? playground.loader.run(playground.modules, compiled.artifact) : null;
      samples.set(id, { source, compiled, result });
    }
    return samples.get(id);
  }
  const hex = (bytes) => [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join(' ');
  const ascii = (bytes) => [...bytes].map((byte) => (byte >= 0x20 && byte < 0x7f ? String.fromCharCode(byte) : '.')).join('');
  const readJson = async (name) => JSON.parse(await readFile(path.join(srcDir, 'content', name), 'utf8'));
  const highlightJson = (value) => escapeHtml(JSON.stringify(value, null, 2)).replace(/^(\s*)(&quot;[^&]*?&quot;)(:)/gm, '$1<span class="k">$2</span>$3');
  const dynamic = {
    'ratio-chart': async () => renderRatioChart(await readJson('ratios.json')),
    'inspect-command': async () => escapeHtml((await readJson('inspect-example.json')).command),
    'inspect-program': async () => highlightGuion((await readJson('inspect-example.json')).program),
    'inspect-manifest': async () => escapeHtml((await readJson('inspect-example.json')).manifest),
    'inspect-report': async () => highlightJson((await readJson('inspect-example.json')).report),
    'cli-version': async () => cli.version,
    'cli-count': async () => String(cli.commands.length),
    'claim-count': async () => String(claims.length),
    'report-kinds': async () => cli.commands.filter((command) => command.report).map((command) => `<li><code>${escapeHtml(command.report)}</code> <span>guion ${escapeHtml(command.name)} --json</span></li>`).join('\n'),
    'cli-table': async () => cli.commands.map((command) => `<tr><th scope="row"><code>guion ${escapeHtml(command.name)}</code></th><td><code>${escapeHtml(command.args || '—')}</code></td><td>${escapeHtml(command.dev)}</td><td>${command.report ? `<code>${escapeHtml(command.report)}</code>` : '—'}</td></tr>`).join('\n'),
    'sample-options': async () => sampleList.map((entry) => `<option value="${escapeHtml(entry.id)}">${escapeHtml(entry.title)}</option>`).join(''),
    'sample-raw': async (id) => escapeHtml((await sampleFacts(id)).source),
    'sample-code': async (id) => highlightGuion((await sampleFacts(id)).source),
    'sample-out': async (id) => escapeHtml((await sampleFacts(id)).result?.stdout ?? ''),
    'sample-facts': async (id) => { const { compiled, result } = await sampleFacts(id); return `${compiled.artifact.byteLength} artifact bytes · ${result.steps} steps`; },
    'hello-hex': async (row) => hex((await sampleFacts('hello')).compiled.artifact.slice(Number(row) * 8, Number(row) * 8 + 8)),
    'hello-ascii': async (row) => ascii((await sampleFacts('hello')).compiled.artifact.slice(Number(row) * 8, Number(row) * 8 + 8)),
    'sample-steps': async (id) => String((await sampleFacts(id)).result.steps),
    'hello-size': async () => String((await sampleFacts('hello')).compiled.artifact.byteLength),
  };
  const cache = new Map();
  const read = async (relative) => {
    if (!cache.has(relative)) cache.set(relative, await readFile(path.join(srcDir, relative), 'utf8'));
    return cache.get(relative);
  };

  // Rules that markup tokens need, such as the case-file links; the same for every page.
  const generatedCss = new Set();

  async function resolve(text, context, report) {
    let current = text;
    for (let pass = 0; pass < 12; pass += 1) {
      const tokens = [...current.matchAll(TOKEN)].filter(([, name]) => name !== 'css');
      if (tokens.length === 0) break;
      const replacements = new Map();
      for (const [whole, name, argument] of tokens) {
        if (replacements.has(whole)) continue;
        let value;
        if (name === 'include' && argument) {
          value = existsSync(path.join(srcDir, argument)) ? await read(argument) : undefined;
        } else if (name === 'slot' && argument) {
          const file = path.join(vendorDir, 'fragments', `${argument}.html`);
          value = existsSync(file) ? (await readFile(file, 'utf8')).trim() : undefined;
        } else if (name === 'claim' && argument) {
          value = claimById.has(argument) ? renderClaimCard(claimById.get(argument), { base: context.base }) : undefined;
        } else if (name === 'casefiles' && argument) {
          const ids = argument.trim().split(/\s+/);
          if (ids.every((id) => claimById.has(id))) {
            const files = renderCaseFiles(ids.map((id) => claimById.get(id)), { base: context.base });
            generatedCss.add(files.css);
            value = files.markup;
          }
        } else if (name === 'report' && argument) {
          value = report[argument];
        } else if (dynamic[name]) {
          value = await dynamic[name](argument);
        } else {
          value = context[name];
        }
        if (value === undefined) throw new Error(`unresolved token: ${whole}`);
        replacements.set(whole, value);
      }
      current = current.replace(TOKEN, (whole, name) => (name === 'css' ? whole : replacements.get(whole)));
    }
    const left = [...current.matchAll(TOKEN)].find(([, name]) => name !== 'css');
    if (left) throw new Error(`unresolved token: ${left[0]}`);
    return current.replace('{{css}}', () => css + [...generatedCss].join(''));
  }

  const pages = [
    { file: 'index.html', context: { base: '', root: './', robots: '', 'plain-checked': ' checked', 'dev-checked': '' } },
    { file: 'developer/index.html', context: { base: '../', root: '../', robots: '<meta name="robots" content="noindex">\n', 'plain-checked': '', 'dev-checked': ' checked' } },
  ];
  const render = (report) => Promise.all(pages.map(async (page) => resolve(shell, page.context, report)));

  // The footer reports the page's own weight; iterate until the rounded figures stop moving.
  let report = { htmlKB: '0 KB', cssKB: '0 KB', jsKB: '0.0 KB', wasmKB: '0.0 MB', remoteRequests: '0' };
  let rendered = await render(report);
  let measured = null;
  for (let pass = 0; pass < 6; pass += 1) {
    const main = rendered[0];
    const style = main.match(/<style>([\s\S]*?)<\/style>/)[1];
    measured = {
      htmlBytes: Buffer.byteLength(main) - Buffer.byteLength(style),
      cssBytes: Buffer.byteLength(style),
      jsBytes,
      wasmBytes,
      remoteRequests: rendered.reduce((total, page) => total + countRemoteRequests(page), 0),
    };
    const next = {
      htmlKB: kb(measured.htmlBytes),
      cssKB: kb(measured.cssBytes),
      jsKB: `${(jsBytes / 1024).toFixed(1)} KB`,
      wasmKB: `${(wasmBytes / 1048576).toFixed(1)} MB`,
      remoteRequests: String(measured.remoteRequests),
    };
    if (JSON.stringify(next) === JSON.stringify(report)) break;
    report = next;
    rendered = await render(report);
  }

  await mkdir(outDir, { recursive: true });
  await rm(path.join(outDir, 'evidence'), { recursive: true, force: true });
  await rm(path.join(outDir, 'playground', 'samples'), { recursive: true, force: true });
  for (const [index, page] of pages.entries()) {
    await mkdir(path.dirname(path.join(outDir, page.file)), { recursive: true });
    await writeFile(path.join(outDir, page.file), rendered[index]);
  }
  await writeFile(path.join(outDir, '.nojekyll'), '');
  // The logo and link-preview card come from the repository's brand/ folder; the page head links each one.
  await rm(path.join(outDir, 'brand'), { recursive: true, force: true });
  if (existsSync(path.join(repoRoot, 'brand'))) {
    await mkdir(path.join(outDir, 'brand'), { recursive: true });
    for (const name of BRAND_FILES) await cp(path.join(repoRoot, 'brand', name), path.join(outDir, 'brand', name));
  }
  await writeFile(path.join(outDir, 'sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n  <url><loc>${SITE_URL}</loc></url>\n  <url><loc>${SITE_URL}evidence/</loc></url>\n</urlset>\n`);
  if (existsSync(loaderFile)) {
    await mkdir(path.join(outDir, 'playground'), { recursive: true });
    await cp(loaderFile, path.join(outDir, 'playground', 'loader.mjs'));
    if (existsSync(path.join(srcDir, 'playground', 'samples'))) {
      await cp(path.join(srcDir, 'playground', 'samples'), path.join(outDir, 'playground', 'samples'), { recursive: true });
    }
  }
  if (path.resolve(wasmDir) !== path.resolve(outDir, 'playground') && existsSync(wasmDir)) {
    await mkdir(path.join(outDir, 'playground'), { recursive: true });
    for (const name of await readdir(wasmDir)) {
      if (name.endsWith('.wasm') || name === 'wasm.build.json') await cp(path.join(wasmDir, name), path.join(outDir, 'playground', name));
    }
  }
  await publishEvidence({ repoRoot, claims, outDir: path.join(outDir, 'evidence') });
  if (cli) {
    const gaps = await pythonGaps(srcDir);
    await writeFile(path.join(outDir, 'agents.json'), renderAgentsJson({ claims, cli, gaps }));
    await writeFile(path.join(outDir, 'llms.txt'), renderLlmsTxt({ claims, gaps }));
  }

  return { files: await walk(outDir), report: measured };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const outDir = path.join(SITE_ROOT, 'public');
  const { files, report } = await buildSite({
    srcDir: path.join(SITE_ROOT, 'src'),
    vendorDir: path.join(SITE_ROOT, 'vendor', 'allhere'),
    outDir,
  });
  console.log(`Built ${files.length} files into site/public (HTML ${report.htmlBytes} B, CSS ${report.cssBytes} B, JS ${report.jsBytes} B, Wasm ${report.wasmBytes} B, remote requests ${report.remoteRequests})`);
}
