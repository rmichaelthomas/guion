// The registry protocol (docs/registry-protocol.md) over any R2-shaped store.
//
// Storage keys (all under one bucket):
//   objects/<sha256>.tar          package source archive, named by its own hash
//   index/<name>.json             public: versions of one package, derived from the records below
//   log/<seq>.json, log/head.json public: append-only, hash-chained record of every change, each
//                                 entry and the head signed with the registry's ed25519 key
//   versions/<name>/<version>.json  the version's immutable claim (the yanked flag is the only mutable field)
//   owners/<name>.json            who owns the name (never served)
//   tokens/<sha256(token)>.json   publisher tokens, stored only as hashes (never served)
//   publisher-tokens/<publisher>/<sha256(token)>  which token hashes a publisher holds (never served)

import { collisionKey, compareVersions, nameProblem, parseVersion } from "./names.js";

export const MAX_ARCHIVE_BYTES = 32 * 1024 * 1024;
const MAX_META_BYTES = 16 * 1024;
const PUBLIC_PREFIXES = ["objects/", "index/", "log/"];
const JSON_HEADERS = { "content-type": "application/json", "cache-control": "no-store" };

class HttpError extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

const json = (body, status = 200, extra = {}) =>
  new Response(JSON.stringify(body), { status, headers: { ...JSON_HEADERS, ...extra } });

export async function sha256Hex(data) {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

const pad = (seq) => String(seq).padStart(10, "0");

const hexToBytes = (hex) => Uint8Array.from(hex.match(/../g), (pair) => parseInt(pair, 16));
const bytesToHex = (bytes) => [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
// PKCS#8 wrapping of a raw 32-byte ed25519 seed.
const ED25519_PKCS8_PREFIX = "302e020100300506032b657004220420";

/** The bytes the registry signs for log entry `seq` whose hash is `hash`. */
export const logSigningMessage = (seq, hash) => new TextEncoder().encode(`guion-registry-log-v1:${seq}:${hash}`);

/**
 * Loads the signing key from `env.SIGNING_KEY` (64 hex characters: the ed25519 seed). Without
 * one the registry refuses to write, so its log never holds an unsigned entry.
 */
async function loadSigner(env) {
  const seed = (env.SIGNING_KEY ?? "").trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(seed)) {
    throw new HttpError(503, "E_SIGNING_UNAVAILABLE", "the registry has no signing key configured; writes are disabled");
  }
  const key = await crypto.subtle.importKey("pkcs8", hexToBytes(ED25519_PKCS8_PREFIX + seed), { name: "Ed25519" }, true, ["sign"]);
  const jwk = await crypto.subtle.exportKey("jwk", key);
  const publicKey = Uint8Array.from(atob(jwk.x.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
  const keyId = (await sha256Hex(publicKey)).slice(0, 16);
  return {
    keyId,
    publicKey: bytesToHex(publicKey),
    sign: async (seq, hash) => bytesToHex(await crypto.subtle.sign({ name: "Ed25519" }, key, logSigningMessage(seq, hash))),
  };
}

async function readJson(store, key) {
  const object = await store.get(key);
  return object ? JSON.parse(await object.text()) : null;
}

/** Creates `key` only if it does not exist. Returns false when someone else got there first. */
async function createOnce(store, key, value) {
  const written = await store.put(key, value, { onlyIf: { etagDoesNotMatch: "*" } });
  return written !== null && written !== undefined;
}

async function authenticateToken(request, store) {
  const header = request.headers.get("authorization") ?? "";
  const match = /^Bearer (\S+)$/.exec(header);
  if (!match) throw new HttpError(401, "E_AUTH_REQUIRED", "a publisher token is required (Authorization: Bearer ...)");
  const hash = await sha256Hex(match[1]);
  const record = await readJson(store, `tokens/${hash}.json`);
  if (!record) throw new HttpError(401, "E_AUTH_INVALID", "the publisher token is not recognised");
  return { publisher: record.publisher, hash };
}

async function authenticate(request, store) {
  return (await authenticateToken(request, store)).publisher;
}

function requireAdmin(request, env) {
  const header = request.headers.get("authorization") ?? "";
  const supplied = header.startsWith("Bearer ") ? header.slice(7) : "";
  if (!env.ADMIN_TOKEN || !constantTimeEqual(supplied, env.ADMIN_TOKEN)) {
    throw new HttpError(401, "E_AUTH_INVALID", "administrator token required");
  }
}

/** Issues a new token for `publisher`; the registry keeps only its hash. */
async function issueToken(store, publisher) {
  const random = crypto.getRandomValues(new Uint8Array(32));
  const token = `gpt_${[...random].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  const hash = await sha256Hex(token);
  const created_at = new Date().toISOString();
  await store.put(`tokens/${hash}.json`, JSON.stringify({ publisher, created_at }));
  await store.put(`publisher-tokens/${publisher}/${hash}`, created_at);
  return token;
}

async function revokeTokenHash(store, publisher, hash) {
  await store.delete(`tokens/${hash}.json`);
  await store.delete(`publisher-tokens/${publisher}/${hash}`);
}

async function listKeys(store, prefix) {
  const keys = [];
  let cursor;
  do {
    const page = await store.list({ prefix, cursor });
    keys.push(...page.objects.map((object) => object.key));
    cursor = page.truncated ? page.cursor : undefined;
  } while (cursor);
  return keys;
}

const validPublisherId = (id) => typeof id === "string" && /^[a-z0-9][a-z0-9-]{0,38}$/.test(id);

/** The caller's token stops working. With `rotate`, a replacement is issued first. */
async function retireOwnToken(request, store, rotate) {
  const { publisher, hash } = await authenticateToken(request, store);
  const token = rotate ? await issueToken(store, publisher) : null;
  await revokeTokenHash(store, publisher, hash);
  return rotate
    ? json({ publisher, token, note: "store this token now; the previous one no longer works" })
    : json({ publisher, revoked: 1 });
}

/** Administrator: every token `id` holds stops working. */
async function revokePublisherTokens(request, store, env, id) {
  requireAdmin(request, env);
  if (!validPublisherId(id)) {
    throw new HttpError(400, "E_PUBLISHER_INVALID", "publisher id must be 1 to 39 lowercase letters, digits or '-'");
  }
  const hashes = new Set(
    (await listKeys(store, `publisher-tokens/${id}/`)).map((key) => key.slice(`publisher-tokens/${id}/`.length)),
  );
  // Tokens issued before the per-publisher index existed are found by their own records.
  for (const key of await listKeys(store, "tokens/")) {
    const hash = key.slice("tokens/".length, -".json".length);
    if (!hashes.has(hash) && (await readJson(store, key))?.publisher === id) hashes.add(hash);
  }
  for (const hash of hashes) await revokeTokenHash(store, id, hash);
  return json({ publisher: id, revoked: hashes.size });
}

function constantTimeEqual(left, right) {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let i = 0; i < left.length; i++) difference |= left.charCodeAt(i) ^ right.charCodeAt(i);
  return difference === 0;
}

function cleanMeta(raw) {
  if (raw.length > MAX_META_BYTES) throw new HttpError(413, "E_META_TOO_LARGE", "package metadata is too large");
  let meta;
  try {
    meta = raw ? JSON.parse(raw) : {};
  } catch {
    throw new HttpError(400, "E_META_INVALID", "x-guion-meta must be JSON");
  }
  const isObject = (value) => value && typeof value === "object" && !Array.isArray(value);
  if (!isObject(meta)) throw new HttpError(400, "E_META_INVALID", "x-guion-meta must be a JSON object");
  const dependencies = meta.dependencies ?? {};
  const capabilities = meta.capabilities ?? {};
  if (!isObject(dependencies) || !Object.values(dependencies).every((v) => typeof v === "string")) {
    throw new HttpError(400, "E_META_INVALID", "dependencies must map package names to requirement text");
  }
  if (!isObject(capabilities)) throw new HttpError(400, "E_META_INVALID", "capabilities must be an object");
  // Earlier clients sent an empty `cache` list; precompiled attachments are deferred, so it is ignored.
  return { dependencies, capabilities };
}

/** Appends one signed entry to the hash-chained log and returns it. */
async function appendLog(store, signer, entry) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const head = (await readJson(store, "log/head.json")) ?? { seq: 0, hash: "0".repeat(64) };
    const body = {
      seq: head.seq + 1,
      time: new Date().toISOString(),
      action: entry.action,
      name: entry.name,
      version: entry.version,
      sha256: entry.sha256,
      publisher: entry.publisher,
      prev: head.hash,
    };
    const hash = await sha256Hex(JSON.stringify(body));
    // The signature covers the hash, which covers `prev`, so it vouches for the whole chain so far.
    const signature = await signer.sign(body.seq, hash);
    const stored = { ...body, hash, key_id: signer.keyId, signature };
    if (await createOnce(store, `log/${pad(body.seq)}.json`, JSON.stringify(stored))) {
      await store.put("log/head.json", JSON.stringify({ seq: body.seq, hash, key_id: signer.keyId, signature }), {
        httpMetadata: { cacheControl: "no-store" },
      });
      return stored;
    }
  }
  throw new HttpError(503, "E_LOG_BUSY", "the log is busy; retry the request");
}

async function rebuildIndex(store, key) {
  const owner = await readJson(store, `owners/${key}.json`);
  const versions = [];
  let cursor;
  do {
    const page = await store.list({ prefix: `versions/${key}/`, cursor });
    for (const item of page.objects) {
      const record = await readJson(store, item.key);
      if (record && record.state === "committed") versions.push(record);
    }
    cursor = page.truncated ? page.cursor : undefined;
  } while (cursor);
  versions.sort((a, b) => compareVersions(a.version, b.version));
  const index = {
    schema: 1,
    name: key,
    owner: owner?.owner ?? null,
    versions: versions.map((record) => ({
      version: record.version,
      sha256: record.sha256,
      size: record.size,
      yanked: record.yanked,
      published_at: record.published_at,
      log_seq: record.log_seq,
      capabilities: record.capabilities,
      dependencies: record.dependencies,
      signature: null,
    })),
  };
  await store.put(`index/${key}.json`, JSON.stringify(index), {
    httpMetadata: { contentType: "application/json", cacheControl: "no-cache" },
  });
}

async function publish(request, store, env, nameParam, version) {
  const publisher = await authenticate(request, store);
  const signer = await loadSigner(env);
  const problem = nameProblem(nameParam);
  if (problem) throw new HttpError(400, "E_NAME_INVALID", `${problem}: ${nameParam}`);
  const key = collisionKey(nameParam);
  if (!parseVersion(version)) throw new HttpError(400, "E_VERSION_INVALID", "version must be major.minor.patch");
  const declared = (request.headers.get("x-guion-sha256") ?? "").toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(declared)) throw new HttpError(400, "E_HASH_MISSING", "x-guion-sha256 must be the archive's SHA-256");
  const meta = cleanMeta(request.headers.get("x-guion-meta") ?? "");
  const declaredLength = Number(request.headers.get("content-length") ?? "0");
  if (declaredLength > MAX_ARCHIVE_BYTES) throw new HttpError(413, "E_ARCHIVE_TOO_LARGE", "the archive is too large");
  const body = new Uint8Array(await request.arrayBuffer());
  if (body.length === 0) throw new HttpError(400, "E_ARCHIVE_EMPTY", "the archive is empty");
  if (body.length > MAX_ARCHIVE_BYTES) throw new HttpError(413, "E_ARCHIVE_TOO_LARGE", "the archive is too large");
  if ((await sha256Hex(body)) !== declared) {
    throw new HttpError(400, "E_HASH_MISMATCH", "the archive does not match the declared SHA-256");
  }

  // Name ownership: first publish claims the name.
  let owner = await readJson(store, `owners/${key}.json`);
  if (!owner) {
    await createOnce(store, `owners/${key}.json`, JSON.stringify({ owner: publisher, claimed_at: new Date().toISOString() }));
    owner = await readJson(store, `owners/${key}.json`);
  }
  if (owner.owner !== publisher) {
    throw new HttpError(403, "E_NOT_OWNER", `the name '${key}' belongs to another publisher`);
  }

  // The version claim is the immutability lock; a retry of the same publish resumes it.
  const recordKey = `versions/${key}/${version}.json`;
  const fresh = {
    version,
    sha256: declared,
    size: body.length,
    yanked: false,
    published_at: new Date().toISOString(),
    state: "pending",
    log_seq: null,
    ...meta,
  };
  let record = fresh;
  if (!(await createOnce(store, recordKey, JSON.stringify(fresh)))) {
    record = await readJson(store, recordKey);
    if (record.sha256 !== declared || record.state === "committed") {
      throw new HttpError(409, "E_VERSION_EXISTS", `${key} ${version} is already published and cannot change`);
    }
  }

  // Object first (content-addressed, so harmless if the rest fails), then the log (the commit
  // point), then the derived records.
  await store.put(`objects/${declared}.tar`, body, {
    httpMetadata: { contentType: "application/x-tar", cacheControl: "public, max-age=31536000, immutable" },
  });
  const logged = await appendLog(store, signer, { action: "publish", name: key, version, sha256: declared, publisher });
  record = { ...record, state: "committed", log_seq: logged.seq };
  await store.put(recordKey, JSON.stringify(record));
  await rebuildIndex(store, key);
  return json({ name: key, version, sha256: declared, log_seq: logged.seq, log_hash: logged.hash }, 201);
}

async function setYanked(request, store, env, nameParam, version, yanked) {
  const publisher = await authenticate(request, store);
  const signer = await loadSigner(env);
  const key = collisionKey(nameParam);
  const owner = await readJson(store, `owners/${key}.json`);
  if (!owner) throw new HttpError(404, "E_NOT_FOUND", `no package named '${key}'`);
  if (owner.owner !== publisher) throw new HttpError(403, "E_NOT_OWNER", `the name '${key}' belongs to another publisher`);
  const recordKey = `versions/${key}/${version}.json`;
  const record = await readJson(store, recordKey);
  if (!record || record.state !== "committed") throw new HttpError(404, "E_NOT_FOUND", `${key} ${version} is not published`);
  if (record.yanked === yanked) return json({ name: key, version, yanked });
  const logged = await appendLog(store, signer, {
    action: yanked ? "yank" : "unyank",
    name: key,
    version,
    sha256: record.sha256,
    publisher,
  });
  await store.put(recordKey, JSON.stringify({ ...record, yanked }));
  await rebuildIndex(store, key);
  return json({ name: key, version, yanked, log_seq: logged.seq });
}

async function createPublisher(request, store, env) {
  requireAdmin(request, env);
  let body;
  try {
    body = await request.json();
  } catch {
    throw new HttpError(400, "E_BODY_INVALID", "body must be JSON");
  }
  const id = body?.id;
  if (!validPublisherId(id)) {
    throw new HttpError(400, "E_PUBLISHER_INVALID", "publisher id must be 1 to 39 lowercase letters, digits or '-'");
  }
  const token = await issueToken(store, id);
  return json({ publisher: id, token, note: "store this token now; the registry keeps only its hash" }, 201);
}

async function servePublic(request, store, path) {
  if (!PUBLIC_PREFIXES.some((prefix) => path.startsWith(prefix)) || path.includes("..")) {
    throw new HttpError(404, "E_NOT_FOUND", "not found");
  }
  let key = path;
  if (path.startsWith("index/") && path.endsWith(".json")) {
    key = `index/${collisionKey(path.slice("index/".length, -".json".length))}.json`;
  }
  const object = await store.get(key);
  if (!object) throw new HttpError(404, "E_NOT_FOUND", "not found");
  const headers = new Headers();
  headers.set("content-type", key.endsWith(".tar") ? "application/x-tar" : "application/json");
  headers.set(
    "cache-control",
    key.startsWith("objects/") || /^log\/\d+\.json$/.test(key) ? "public, max-age=31536000, immutable" : "no-cache",
  );
  headers.set("etag", `"${object.etag ?? ""}"`);
  return new Response(request.method === "HEAD" ? null : object.body, { status: 200, headers });
}

export async function handle(request, env) {
  const store = env.REGISTRY;
  const url = new URL(request.url);
  const segments = url.pathname.replace(/^\/+/, "").split("/").map((part) => {
    try {
      return decodeURIComponent(part);
    } catch {
      return part;
    }
  });
  const path = segments.join("/");
  try {
    if (request.method === "GET" || request.method === "HEAD") {
      if (path === "" || path === "v1") return json({ service: "guion-registry", protocol: 1 });
      if (path === "v1/signing-key") {
        const signer = await loadSigner(env);
        return json({ algorithm: "ed25519", key_id: signer.keyId, public_key: signer.publicKey });
      }
      return await servePublic(request, store, path);
    }
    const parts = segments;
    if (request.method === "PUT" && parts.length === 4 && parts[0] === "v1" && parts[1] === "publish") {
      return await publish(request, store, env, parts[2], parts[3]);
    }
    if (request.method === "POST" && parts.length === 4 && parts[0] === "v1" && (parts[1] === "yank" || parts[1] === "unyank")) {
      return await setYanked(request, store, env, parts[2], parts[3], parts[1] === "yank");
    }
    if (request.method === "POST" && path === "v1/admin/publishers") {
      return await createPublisher(request, store, env);
    }
    if (request.method === "POST" && parts.length === 5 && path.startsWith("v1/admin/publishers/") && parts[4] === "revoke") {
      return await revokePublisherTokens(request, store, env, parts[3]);
    }
    if (request.method === "POST" && (path === "v1/tokens/rotate" || path === "v1/tokens/revoke")) {
      return await retireOwnToken(request, store, path === "v1/tokens/rotate");
    }
    throw new HttpError(404, "E_NOT_FOUND", "not found");
  } catch (error) {
    if (error instanceof HttpError) {
      return json({ error: { code: error.code, message: error.message } }, error.status);
    }
    return json({ error: { code: "E_INTERNAL", message: "internal error" } }, 500);
  }
}
