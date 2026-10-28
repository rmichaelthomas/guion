const escapeXml = (text) => String(text).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
const fmt = (n) => Number(n.toFixed(2));

const WIDTH = 960;
const LABEL_RIGHT = 326;
const PLOT = { left: 360, right: 920, min: 0.1, max: 1.6 };
const ROW = 34;
const TICKS = [0.1, 0.2, 0.5, 1, 1.5];

const x = (value) => PLOT.left + ((Math.log10(value) - Math.log10(PLOT.min)) / (Math.log10(PLOT.max) - Math.log10(PLOT.min))) * (PLOT.right - PLOT.left);

/**
 * A horizontal ratio chart on a log scale. Each entry is Guion's time divided by the comparison's,
 * with its 95% interval; the line at 1 is "same time as Python". Both directions are plotted.
 */
export function renderRatioChart(entries) {
  const faster = entries.filter((entry) => entry.ratio < 1).sort((a, b) => a.ratio - b.ratio);
  const slower = entries.filter((entry) => entry.ratio >= 1).sort((a, b) => a.ratio - b.ratio);
  const top = 64;
  const groups = [['Guion faster', faster, 'fast'], ['Guion slower', slower, 'slow']].filter(([, rows]) => rows.length > 0);
  const height = top + groups.reduce((sum, [, rows]) => sum + 36 + rows.length * ROW, 0) + 58;
  const one = fmt(x(1));

  const parts = [];
  parts.push(`<svg class="ratio-chart" viewBox="0 0 ${WIDTH} ${height}" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="ratio-title ratio-desc">`);
  parts.push(`<title id="ratio-title">Guion's time as a fraction of the comparison's time, on a log scale</title>`);
  parts.push(`<desc id="ratio-desc">Each dot is the ratio of Guion's median time to the comparison's, with its 95% interval as a line. A dot left of the dashed line means Guion took less time; right of it means Guion took more.</desc>`);
  parts.push(`<rect class="band-fast" x="${PLOT.left}" y="${top - 18}" width="${fmt(one - PLOT.left)}" height="${height - top - 40}"/>`);
  parts.push(`<rect class="band-slow" x="${one}" y="${top - 18}" width="${fmt(PLOT.right - one)}" height="${height - top - 40}"/>`);
  parts.push(`<text class="band-label" x="${fmt(one - 10)}" y="${top - 30}" text-anchor="end">← Guion faster</text>`);
  parts.push(`<text class="band-label" x="${fmt(Number(one) + 10)}" y="${top - 30}">Guion slower →</text>`);
  for (const tick of TICKS) {
    parts.push(`<line class="grid" x1="${fmt(x(tick))}" x2="${fmt(x(tick))}" y1="${top - 18}" y2="${height - 40}"/>`);
    parts.push(`<text class="tick" x="${fmt(x(tick))}" y="${height - 20}" text-anchor="middle">${tick}×</text>`);
  }
  parts.push(`<line data-reference="1" class="reference" x1="${one}" x2="${one}" y1="${top - 22}" y2="${height - 40}"/>`);
  parts.push(`<text class="reference-label" x="${one}" y="${height - 4}" text-anchor="middle">same time as Python</text>`);

  let y = top;
  for (const [title, rows, kind] of groups) {
    parts.push(`<text class="group" x="20" y="${y + 18}">${escapeXml(title)}</text>`);
    y += 36;
    for (const entry of rows) {
      const cy = y + ROW / 2 - 4;
      parts.push(`<g class="row row-${kind}">`);
      parts.push(`<text class="row-label" x="${LABEL_RIGHT}" y="${cy + 4}" text-anchor="end">${escapeXml(entry.label)}</text>`);
      parts.push(`<line class="whisker" x1="${fmt(x(entry.low))}" x2="${fmt(x(entry.high))}" y1="${cy}" y2="${cy}"/>`);
      parts.push(`<circle data-id="${escapeXml(entry.id)}" class="dot dot-${kind}" cx="${fmt(x(entry.ratio))}" cy="${cy}" r="6.5"/>`);
      parts.push(`<text class="value" x="${fmt(x(entry.high) + 12)}" y="${cy + 4}">${entry.ratio.toFixed(3)}×</text>`);
      parts.push('</g>');
      y += ROW;
    }
  }
  parts.push('</svg>');
  return parts.join('\n');
}
