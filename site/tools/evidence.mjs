import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const escapeHtml = (text) => String(text)
  .replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');

/**
 * Copy the files that claims cite into `outDir` (flat, by file name) and write `index.html`
 * listing each with its size and SHA-256, the claims that cite it, and how to reproduce it.
 */
export async function publishEvidence({ repoRoot, claims, outDir }) {
  const bySource = new Map();
  for (const claim of claims) {
    for (const source of claim.evidence) {
      if (!bySource.has(source)) bySource.set(source, { titles: [], commands: [] });
      const record = bySource.get(source);
      record.titles.push(claim.title);
      if (claim.dev?.command) record.commands.push(claim.dev.command);
    }
  }

  const byName = new Map();
  for (const source of bySource.keys()) {
    const name = path.basename(source);
    if (byName.has(name)) throw new Error(`two evidence files have the same name "${name}": ${byName.get(name)} and ${source}`);
    byName.set(name, source);
  }

  await mkdir(outDir, { recursive: true });
  const entries = [];
  for (const [name, sourcePath] of [...byName].sort(([a], [b]) => a.localeCompare(b))) {
    let bytes;
    try {
      bytes = await readFile(path.join(repoRoot, sourcePath));
    } catch {
      throw new Error(`evidence file not found: ${sourcePath}`);
    }
    await writeFile(path.join(outDir, name), bytes);
    const record = bySource.get(sourcePath);
    entries.push({
      name,
      sourcePath,
      bytes: bytes.byteLength,
      sha256: createHash('sha256').update(bytes).digest('hex'),
      cites: [...new Set(record.titles)],
      commands: [...new Set(record.commands)],
    });
  }

  const rows = entries.map((entry) => `<tr>
<td><a href="${escapeHtml(entry.name)}">${escapeHtml(entry.name)}</a></td>
<td class="num">${entry.bytes.toLocaleString('en-US')}</td>
<td><code>${entry.sha256}</code></td>
<td>${entry.cites.map(escapeHtml).join('; ')}</td>
<td>${entry.commands.map((command) => `<code>${escapeHtml(command)}</code>`).join('<br>') || '<span class="muted">see the report</span>'}</td>
</tr>`).join('\n');

  await writeFile(path.join(outDir, 'index.html'), `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Guion evidence files</title>
<style>
:root{color-scheme:light;--paper:#e9ede3;--ink:#11251d;--blue:#2148e8;--muted:#607569;--line:#afbaad}
body{margin:0;background:var(--paper);color:var(--ink);font:16px/1.6 ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif}
main{width:min(1200px,100%);margin:auto;padding:clamp(1.2rem,4vw,3rem)}
h1{font-size:clamp(2rem,5vw,3.4rem);line-height:1;letter-spacing:-.04em;margin:.4em 0}
a{color:var(--blue);text-underline-offset:.2em}a:focus-visible{outline:3px solid var(--blue);outline-offset:3px}
.table-wrap{overflow-x:auto}
table{border-collapse:collapse;width:100%;font-size:.85rem}
th,td{text-align:left;vertical-align:top;padding:.6rem .7rem;border-bottom:1px solid var(--line)}
code{font:.78rem ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;overflow-wrap:anywhere}
.num{text-align:right;font-variant-numeric:tabular-nums}.muted{color:var(--muted)}
</style>
</head>
<body>
<main>
<p><a href="../index.html">← guion</a></p>
<h1>The files behind the numbers</h1>
<p>Every figure on the Guion page comes from one of these reports or raw-sample files. Each is a byte-for-byte copy of a file in the project repository, listed with its size and SHA-256. Check a hash with <code>shasum -a 256 FILE</code>. The measurements describe named workloads on one machine; read each report's own limits before you rely on a number.</p>
<div class="table-wrap">
<table>
<thead><tr><th scope="col">File</th><th scope="col">Bytes</th><th scope="col">SHA-256</th><th scope="col">Cited by</th><th scope="col">Reproduce with</th></tr></thead>
<tbody>
${rows}
</tbody>
</table>
</div>
</main>
</body>
</html>
`);
  return { entries };
}
