import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

export const CONTACT = 'rmichael@rmichaelthomas.com';

const escapeHtml = (text) => String(text)
  .replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');

/** Reports use Markdown emphasis and wrapped lines; compare on the words and numbers only. */
export const normalize = (text) => text.replace(/[*`]/g, '').replace(/\s+/g, ' ');

export function loadClaims(srcDir) {
  const claims = JSON.parse(readFileSync(path.join(srcDir, 'claims.json'), 'utf8'));
  if (!Array.isArray(claims)) throw new TypeError('claims.json must be an array');
  const seen = new Set();
  for (const claim of claims) {
    const id = claim?.id;
    if (typeof id !== 'string' || !/^[a-z0-9-]+$/.test(id)) throw new TypeError(`claim has a bad id: ${JSON.stringify(id)}`);
    if (seen.has(id)) throw new TypeError(`duplicate claim id: ${id}`);
    seen.add(id);
    for (const [field, ok] of [
      ['title', typeof claim.title === 'string' && claim.title !== ''],
      ['plain', ['say', 'holdsWhen', 'doesntTell'].every((key) => typeof claim.plain?.[key] === 'string' && claim.plain[key] !== '')],
      ['dev.rows', Array.isArray(claim.dev?.rows)],
      ['figures', Array.isArray(claim.figures)],
      ['sources', Array.isArray(claim.sources) && claim.sources.length > 0],
      ['evidence', Array.isArray(claim.evidence)],
    ]) {
      if (!ok) throw new TypeError(`claim ${id}: invalid ${field}`);
    }
  }
  return claims;
}

const NUMBER = /\d[\d,]*(?:\.\d+)?/g;

function numbersIn(text) {
  const tokens = [];
  for (const [raw] of text.matchAll(NUMBER)) {
    const token = raw.replace(/,+$/, '');
    // Short whole numbers ("15 trials", "2") are too common to prove anything.
    if (token.length >= 3 || /[.,]/.test(token)) tokens.push(token);
  }
  return tokens;
}

/** Empty array means every figure, condition, and number in the claim's prose is in its sources. */
export function checkClaims(claims, repoRoot) {
  const issues = [];
  const cache = new Map();
  const read = (relative) => {
    if (!cache.has(relative)) {
      const file = path.join(repoRoot, relative);
      cache.set(relative, existsSync(file) ? normalize(readFileSync(file, 'utf8')) : null);
    }
    return cache.get(relative);
  };
  for (const claim of claims) {
    const texts = [];
    for (const source of claim.sources) {
      const text = read(source.path);
      if (text === null) {
        issues.push({ id: claim.id, reason: `missing source: ${source.path}` });
        continue;
      }
      texts.push(text);
      for (const needle of source.needles ?? []) {
        if (!text.includes(normalize(needle))) issues.push({ id: claim.id, reason: `needle not found: ${needle}` });
      }
    }
    const inSources = (needle) => texts.some((text) => text.includes(normalize(needle)));
    for (const figure of claim.figures) {
      if (!inSources(figure)) issues.push({ id: claim.id, reason: `figure not found: ${figure}` });
    }
    const prose = [claim.title, ...Object.values(claim.plain), ...claim.dev.rows.flat()].join(' ');
    for (const token of new Set(numbersIn(prose))) {
      if (!inSources(token)) issues.push({ id: claim.id, reason: `number not in any source: ${token}` });
    }
  }
  return issues;
}

/**
 * Several claims as one case-file viewer: a numbered index of radio labels and a panel that
 * shows the chosen receipt. Returns the markup and the per-claim rules that link them; where
 * `:has()` is unsupported the rules do not apply and every receipt shows in order.
 */
export function renderCaseFiles(claims, { base = '' } = {}) {
  const number = (index) => String(index + 1).padStart(2, '0');
  const radios = claims
    .map((claim, index) => `<input type="radio" name="case-file" id="case-${escapeHtml(claim.id)}" class="sr-only"${index === 0 ? ' checked' : ''}>`)
    .join('\n');
  const tabs = claims
    .map((claim, index) => `<label for="case-${escapeHtml(claim.id)}" class="case-tab" data-group="${escapeHtml(claim.group ?? '')}"><span class="case-n">${number(index)}</span><span class="case-title">${escapeHtml(claim.title)}</span></label>`)
    .join('\n');
  const cards = claims
    .map((claim, index) => renderClaimCard(claim, { base, caseNumber: number(index) }))
    .join('\n');
  const markup = `<div class="casefiles" data-ahi="ahi.reveals-discoveries.tabbed-scene">
${radios}
<div class="case-index" role="group" aria-label="Case files">
${tabs}
</div>
<div class="case-panels" data-ahi="ahi.publications-objects.case-dossier">
${cards}
</div>
</div>`;
  const rules = claims.map((claim) => {
    const id = claim.id;
    return `.casefiles:has(#case-${id}:checked) #claim-${id} { display: flex; }
.casefiles:has(#case-${id}:checked) label[for="case-${id}"] { background: var(--ink); color: var(--lily); border-color: var(--ink); }
.casefiles:has(#case-${id}:checked) label[for="case-${id}"] .case-n { color: var(--lily); }
.casefiles:has(#case-${id}:focus-visible) label[for="case-${id}"] { outline: 3px solid var(--blue); outline-offset: 2px; }
@media (forced-colors: active) { .casefiles:has(#case-${id}:checked) label[for="case-${id}"] { outline: 3px solid Highlight; } }`;
  });
  const css = `@layer components {\n@supports selector(:has(*)) {\n.case-panels > .claim { display: none; }\n${rules.join('\n')}\n}\n}\n`;
  return { markup, css };
}

export function renderClaimCard(claim, { base = '', caseNumber } = {}) {
  const evidenceLinks = claim.evidence
    .map((file) => `<a href="${base}evidence/${escapeHtml(path.basename(file))}">${escapeHtml(path.basename(file))}</a>`)
    .join(' · ');
  const subject = encodeURIComponent(`Guion claim: ${claim.title}`);
  const rows = claim.dev.rows
    .map(([label, value]) => `<tr><th scope="row">${escapeHtml(label)}</th><td>${escapeHtml(value)}</td></tr>`)
    .join('');
  const command = claim.dev.command ? `<pre class="claim-command"><code>${escapeHtml(claim.dev.command)}</code></pre>` : '';
  const caseAttribute = caseNumber ? ` data-case="${escapeHtml(caseNumber)}"` : '';
  return `<article class="claim" id="claim-${escapeHtml(claim.id)}" data-claim="${escapeHtml(claim.id)}" data-group="${escapeHtml(claim.group ?? '')}"${caseAttribute}>
<h3>${escapeHtml(claim.title)}</h3>
<dl class="receipt">
<div><dt>Claim</dt><dd>${escapeHtml(claim.plain.say)}</dd></div>
<div><dt>Holds when</dt><dd>${escapeHtml(claim.plain.holdsWhen)}</dd></div>
<div><dt>Doesn't tell you</dt><dd>${escapeHtml(claim.plain.doesntTell)}</dd></div>
<div><dt>Check it</dt><dd>${evidenceLinks || 'Described in the repository documents.'}</dd></div>
<div><dt>Say it's wrong</dt><dd><a href="mailto:${CONTACT}?subject=${subject}">${CONTACT}</a></dd></div>
</dl>
<div class="v-dev"><table class="claim-table"><tbody>${rows}</tbody></table>${command}</div>
</article>`;
}
