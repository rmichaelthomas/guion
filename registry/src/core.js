// The registry protocol (docs/registry-protocol.md) over any R2-shaped store.
//
// Storage keys (all under one bucket):
//   objects/<sha256>.tar          package source archive, named by its own hash
//   index/<name>.json             public: versions of one package, derived from the records below
//   log/<seq>.json, log/head.json public: append-only, hash-chained record of every change
//   versions/<name>/<version>.json  the version's immutable claim (the yanked flag is the only mutable field)
//   owners/<name>.json            who owns the name (never served)
//   tokens/<sha256(token)>.json   publisher tokens, stored only as hashes (never served)

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

async function readJson(store, key) {
  const object = await store.get(key);
  return object ? JSON.parse(await object.text()) : null;
}

/** Creates `key` only if it does not exist. Returns false when someone else got there first. */
async function createOnce(store, key, value) {
  const written = await store.put(key, value, { onlyIf: { etagDoesNotMatch: "*" } });
  return written !== null && written !== undefined;
}

async function authenticate(request, store) {
  const header = request.headers.get("authorization") ?? "";
  const match = /^Bearer (\S+)$/.exec(header);
  if (!match) throw new HttpError(401, "E_AUTH_REQUIRED", "a publisher token is required (Authorization: Bearer ...)");
  const record = await readJson(store, `tokens/${await sha256Hex(match[1])}.json`);
  if (!record) throw new HttpError(401, "E_AUTH_INVALID", "the publisher token is not recognised");
  return record.publisher;
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
  const cache = meta.cache ?? [];
  if (!isObject(dependencies) || !Object.values(dependencies).every((v) => typeof v === "string")) {
    throw new HttpError(400, "E_META_INVALID", "dependencies must map package names to requirement text");
  }
  if (!isObject(capabilities)) throw new HttpError(400, "E_META_INVALID", "capabilities must be an object");
  if (
    !Array.isArray(cache) ||
    !cache.every((entry) => isObject(entry) && typeof entry.compiler === "string" && /^[0-9a-f]{64}$/.test(entry.sha256 ?? ""))
  ) {
    throw new HttpError(400, "E_META_INVALID", "cache attachments need a compiler version and a sha256");
  }
  return { dependencies, capabilities, cache };
}

/** Appends one entry to the hash-chained log and returns it. */
async function appendLog(store, entry) {
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
    const stored = { ...body, hash };
    if (await createOnce(store, `log/${pad(body.seq)}.json`, JSON.stringify(stored))) {
      await store.put("log/head.json", JSON.stringify({ seq: body.seq, hash }), {
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
      cache: record.cache,
      signature: null,
    })),
  };
  await store.put(`index/${key}.json`, JSON.stringify(index), {
    httpMetadata: { contentType: "application/json", cacheControl: "no-cache" },
  });
}

async function publish(request, store, nameParam, version) {
  const publisher = await authenticate(request, store);
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
  const logged = await appendLog(store, { action: "publish", name: key, version, sha256: declared, publisher });
  record = { ...record, state: "committed", log_seq: logged.seq };
  await store.put(recordKey, JSON.stringify(record));
  await rebuildIndex(store, key);
  return json({ name: key, version, sha256: declared, log_seq: logged.seq, log_hash: logged.hash }, 201);
}

async function setYanked(request, store, nameParam, version, yanked) {
  const publisher = await authenticate(request, store);
  const key = collisionKey(nameParam);
  const owner = await readJson(store, `owners/${key}.json`);
  if (!owner) throw new HttpError(404, "E_NOT_FOUND", `no package named '${key}'`);
  if (owner.owner !== publisher) throw new HttpError(403, "E_NOT_OWNER", `the name '${key}' belongs to another publisher`);
  const recordKey = `versions/${key}/${version}.json`;
  const record = await readJson(store, recordKey);
  if (!record || record.state !== "committed") throw new HttpError(404, "E_NOT_FOUND", `${key} ${version} is not published`);
  if (record.yanked === yanked) return json({ name: key, version, yanked });
  const logged = await appendLog(store, {
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
  const header = request.headers.get("authorization") ?? "";
  const supplied = header.startsWith("Bearer ") ? header.slice(7) : "";
  if (!env.ADMIN_TOKEN || !constantTimeEqual(supplied, env.ADMIN_TOKEN)) {
    throw new HttpError(401, "E_AUTH_INVALID", "administrator token required");
  }
  let body;
  try {
    body = await request.json();
  } catch {
    throw new HttpError(400, "E_BODY_INVALID", "body must be JSON");
  }
  const id = body?.id;
  if (typeof id !== "string" || !/^[a-z0-9][a-z0-9-]{0,38}$/.test(id)) {
    throw new HttpError(400, "E_PUBLISHER_INVALID", "publisher id must be 1 to 39 lowercase letters, digits or '-'");
  }
  const random = crypto.getRandomValues(new Uint8Array(32));
  const token = `gpt_${[...random].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  await store.put(`tokens/${await sha256Hex(token)}.json`, JSON.stringify({ publisher: id, created_at: new Date().toISOString() }));
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
      return await servePublic(request, store, path);
    }
    const parts = segments;
    if (request.method === "PUT" && parts.length === 4 && parts[0] === "v1" && parts[1] === "publish") {
      return await publish(request, store, parts[2], parts[3]);
    }
    if (request.method === "POST" && parts.length === 4 && parts[0] === "v1" && (parts[1] === "yank" || parts[1] === "unyank")) {
      return await setYanked(request, store, parts[2], parts[3], parts[1] === "yank");
    }
    if (request.method === "POST" && path === "v1/admin/publishers") {
      return await createPublisher(request, store, env);
    }
    throw new HttpError(404, "E_NOT_FOUND", "not found");
  } catch (error) {
    if (error instanceof HttpError) {
      return json({ error: { code: error.code, message: error.message } }, error.status);
    }
    return json({ error: { code: "E_INTERNAL", message: "internal error" } }, 500);
  }
}
