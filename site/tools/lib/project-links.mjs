/**
 * The page may link to the project's own GitHub repository and releases. A link is navigation,
 * not a request: nothing is fetched until a visitor follows it. Every other absolute address,
 * and any such address used as a script, image or stylesheet, is still refused.
 */
export const PROJECT_LINK = /<a\b[^>]*\bhref="https:\/\/github\.com\/rmichaelthomas\/guion(?:\/[^"]*)?"[^>]*>/g;

export const withoutProjectLinks = (html) => html.replace(PROJECT_LINK, '<a>');
