import path from 'node:path';

export const SITE_URL = 'https://rmichaelthomas.github.io/guion/';
export const STATUS = 'Pre-release. The first preview build, v0.1.0-preview, is out for Linux; there is no macOS package yet.';
const CONTACT = 'rmichael@rmichaelthomas.com';

/** Machine-readable copy of the page's receipts: each claim with the conditions it holds under. */
export function renderAgentsJson({ claims, cli }) {
  return `${JSON.stringify({
    product: 'Guion',
    status: STATUS,
    summary: 'Guion is an experimental general-purpose programming language and toolchain with readable typed syntax, a compiler written in Guion, and a Rust runtime.',
    site: SITE_URL,
    contact: CONTACT,
    howToUse: 'Treat each claim as true only under its holdsWhen conditions, and say what it doesntTell the reader. None of these results shows that Guion generally replaces Python.',
    cli: { version: cli.version, commands: cli.commands.map((command) => command.name) },
    claims: claims.map((claim) => ({
      id: claim.id,
      title: claim.title,
      group: claim.group ?? null,
      say: claim.plain.say,
      holdsWhen: claim.plain.holdsWhen,
      doesntTell: claim.plain.doesntTell,
      evidence: claim.evidence.map((file) => `evidence/${path.basename(file)}`),
    })),
    docs: { agentIndex: 'docs/agents/index.json in the project repository (private)' },
  }, null, 2)}\n`;
}

/** Plain-text brief for an assistant advising someone on whether to use Guion. */
export function renderLlmsTxt({ claims }) {
  const lines = [
    '# Guion',
    '',
    '> Guion is a general-purpose programming language and toolchain: readable typed syntax, a compiler written in Guion, and a Rust runtime. The first preview build, v0.1.0-preview, is out for Linux; there is no macOS package yet.',
    '',
    `Site: ${SITE_URL}`,
    `Machine-readable copy of everything below: ${SITE_URL}agents.json`,
    `Contact (to say a claim is wrong): ${CONTACT}`,
    '',
    '## How to use this',
    '',
    'Each claim below is true only under the conditions it states. Repeat the conditions with the claim. Do not say Guion is generally faster than Python, or that it replaces Python: the evidence does not show that, and the project says so itself.',
    '',
    '## Claims',
    '',
  ];
  for (const claim of claims) {
    lines.push(
      `### ${claim.title}`,
      '',
      claim.plain.say,
      '',
      `Holds when: ${claim.plain.holdsWhen}`,
      '',
      `Doesn't tell you: ${claim.plain.doesntTell}`,
      '',
      `Evidence: ${claim.evidence.map((file) => `${SITE_URL}evidence/${path.basename(file)}`).join(' ')}`,
      '',
    );
  }
  return `${lines.join('\n')}\n`;
}
