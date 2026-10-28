const KEYWORDS = new Set(['fn', 'let', 'var', 'set', 'return', 'if', 'else', 'while', 'for', 'in', 'match', 'record', 'import', 'pub', 'and', 'or', 'not', 'true', 'false', 'external', 'continue', 'break']);
const escapeHtml = (text) => text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
const wrap = (kind, text) => `<span class="${kind}">${escapeHtml(text)}</span>`;

/** Minimal, dependency-free highlighter for Guion source. Output text equals the input once tags are removed. */
export function highlightGuion(source) {
  const pattern = /(#[^\n]*)|("(?:\\.|[^"\\\n])*")|(\b\d[\d_]*(?:\.\d+)?\b)|([A-Za-z_][A-Za-z0-9_]*)/g;
  let output = '';
  let last = 0;
  for (const match of source.matchAll(pattern)) {
    output += escapeHtml(source.slice(last, match.index));
    const [text, comment, string, number, word] = match;
    if (comment) output += wrap('c', text);
    else if (string) output += wrap('s', text);
    else if (number) output += wrap('n', text);
    else if (KEYWORDS.has(word)) output += wrap('k', text);
    else if (/^[A-Z]/.test(word)) output += wrap('t', text);
    else output += escapeHtml(text);
    last = match.index + text.length;
  }
  return output + escapeHtml(source.slice(last));
}
