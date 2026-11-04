// Package-name and version rules. These mirror crates/cli/src/package_name.rs; the reserved list
// lives in registry/reserved-names.json and a test in each language keeps it in step with the
// standard library.
import reserved from "../reserved-names.json" with { type: "json" };

export const MAX_NAME_LENGTH = 64;

/** `-` and `_` are the same character, and names are lowercase. */
export function collisionKey(name) {
  return name.toLowerCase().replaceAll("_", "-");
}

export function isReserved(name) {
  const key = collisionKey(name);
  return reserved.exact.includes(key) || reserved.prefixes.some((prefix) => key.startsWith(prefix));
}

/** Returns an error message, or null when the name may be published. */
export function nameProblem(name) {
  if (name.startsWith("@")) {
    return "scoped @owner/name packages are reserved and not available yet";
  }
  if (name.length === 0) return "package name is empty";
  if (name.length > MAX_NAME_LENGTH) return `package name is longer than ${MAX_NAME_LENGTH} characters`;
  if (/[A-Z]/.test(name)) return "package name must be lowercase";
  if (!/^[a-z0-9_-]+$/.test(name)) {
    return "package name may only contain lowercase letters, digits, '-' and '_'";
  }
  if (!/^[a-z]/.test(name)) return "package name must start with a letter";
  if (/[-_]$/.test(name)) return "package name must not end with '-' or '_'";
  if (isReserved(name)) return "package name is reserved";
  return null;
}

export function parseVersion(text) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(text);
  if (!match) return null;
  return match.slice(1).map(Number);
}

export function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] - b[i];
  }
  return 0;
}
