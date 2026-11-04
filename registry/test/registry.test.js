import assert from "node:assert/strict";
import { createHash, createPublicKey, verify } from "node:crypto";
import { readdirSync } from "node:fs";
import { test } from "node:test";
import { handle } from "../src/core.js";
import { collisionKey, compareVersions, nameProblem } from "../src/names.js";
import reserved from "../reserved-names.json" with { type: "json" };
import { MemoryStore } from "./memory-store.js";

const ADMIN = "admin-secret-for-tests";
// A throwaway ed25519 seed; it protects nothing.
const SIGNING_KEY = "11".repeat(32);
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");

function registry() {
  const env = { REGISTRY: new MemoryStore(), ADMIN_TOKEN: ADMIN, SIGNING_KEY };
  const call = (method, path, { token, body, headers = {} } = {}) =>
    handle(
      new Request(`http://registry.test/${path}`, {
        method,
        headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers },
        body,
      }),
      env,
    );
  return { env, call };
}

async function newPublisher(call, id) {
  const response = await call("POST", "v1/admin/publishers", { token: ADMIN, body: JSON.stringify({ id }) });
  assert.equal(response.status, 201);
  return (await response.json()).token;
}

async function publish(call, token, name, version, content, meta = {}) {
  const body = new TextEncoder().encode(content);
  return call("PUT", `v1/publish/${name}/${version}`, {
    token,
    body,
    headers: { "x-guion-sha256": sha(body), "x-guion-meta": JSON.stringify(meta) },
  });
}

test("the reserved list covers every standard-library module", () => {
  const modules = readdirSync(new URL("../../language/stdlib", import.meta.url))
    .filter((file) => file.endsWith(".gu"))
    .map((file) => file.slice(0, -3).replaceAll("_", "-"));
  for (const module of modules) assert.ok(reserved.exact.includes(module), module);
});

test("name rules match the documented ones", () => {
  assert.equal(nameProblem("http-client"), null);
  assert.match(nameProblem("Http"), /lowercase/);
  assert.match(nameProblem("regex"), /reserved/);
  assert.match(nameProblem("guion-tools"), /reserved/);
  assert.match(nameProblem("@acme/widgets"), /reserved/);
  assert.match(nameProblem("x".repeat(65)), /longer than 64/);
  assert.match(nameProblem("9lives"), /start with a letter/);
  assert.equal(collisionKey("Http_Client"), "http-client");
  assert.ok(compareVersions("1.10.0", "1.9.9") > 0);
});

test("publish, download and verify the log chain", async () => {
  const { call } = registry();
  const token = await newPublisher(call, "alice");
  const made = await publish(call, token, "my-lib", "1.0.0", "archive one", { dependencies: { other: "^1.0" } });
  assert.equal(made.status, 201);
  const result = await made.json();
  assert.equal(result.sha256, sha("archive one"));

  const index = await (await call("GET", "index/my-lib.json")).json();
  assert.equal(index.owner, "alice");
  assert.equal(index.versions[0].version, "1.0.0");
  assert.equal(index.versions[0].signature, null);
  assert.deepEqual(index.versions[0].dependencies, { other: "^1.0" });

  const object = await call("GET", `objects/${result.sha256}.tar`);
  assert.equal(sha(new Uint8Array(await object.arrayBuffer())), result.sha256);

  await publish(call, token, "my-lib", "1.1.0", "archive two");
  const head = await (await call("GET", "log/head.json")).json();
  assert.equal(head.seq, 2);
  const second = await (await call("GET", "log/0000000002.json")).json();
  const first = await (await call("GET", "log/0000000001.json")).json();
  assert.equal(second.prev, first.hash);
  const { hash, key_id, signature, ...body } = second;
  assert.equal(hash, sha(JSON.stringify(body)));
  assert.equal(head.hash, hash);

  // Every entry and the head are signed over "guion-registry-log-v1:<seq>:<hash>".
  const published = await (await call("GET", "v1/signing-key")).json();
  assert.equal(published.algorithm, "ed25519");
  assert.equal(key_id, published.key_id);
  assert.equal(published.key_id, sha(Buffer.from(published.public_key, "hex")).slice(0, 16));
  const publicKey = createPublicKey({
    key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), Buffer.from(published.public_key, "hex")]),
    format: "der",
    type: "spki",
  });
  const signed = (seq, entryHash, sig) =>
    verify(null, Buffer.from(`guion-registry-log-v1:${seq}:${entryHash}`), publicKey, Buffer.from(sig, "hex"));
  assert.ok(signed(2, hash, signature));
  assert.ok(signed(1, first.hash, first.signature));
  assert.ok(signed(head.seq, head.hash, head.signature));
  assert.ok(!signed(1, hash, signature), "a signature is bound to its sequence number");
  assert.equal((await (await call("GET", "index/my_lib.json")).json()).versions.length, 2);
});

test("without a signing key the registry refuses writes but still serves reads", async () => {
  const { env, call } = registry();
  const token = await newPublisher(call, "alice");
  assert.equal((await publish(call, token, "my-lib", "1.0.0", "archive one")).status, 201);
  delete env.SIGNING_KEY;
  const refused = await publish(call, token, "my-lib", "1.1.0", "archive two");
  assert.equal(refused.status, 503);
  assert.equal((await refused.json()).error.code, "E_SIGNING_UNAVAILABLE");
  assert.equal((await call("POST", "v1/yank/my-lib/1.0.0", { token })).status, 503);
  assert.equal((await call("GET", "log/head.json")).status, 200);
  assert.equal((await (await call("GET", "log/head.json")).json()).seq, 1);
  assert.equal((await call("GET", "v1/signing-key")).status, 503);
});

test("a published version is immutable", async () => {
  const { call } = registry();
  const token = await newPublisher(call, "alice");
  assert.equal((await publish(call, token, "sample", "1.0.0", "original")).status, 201);
  const again = await publish(call, token, "sample", "1.0.0", "changed");
  assert.equal(again.status, 409);
  const same = await publish(call, token, "sample", "1.0.0", "original");
  assert.equal(same.status, 409);
  const index = await (await call("GET", "index/sample.json")).json();
  assert.equal(index.versions[0].sha256, sha("original"));
});

test("names are owned, and differently-spelled names collide", async () => {
  const { call } = registry();
  const alice = await newPublisher(call, "alice");
  const bob = await newPublisher(call, "bob");
  assert.equal((await publish(call, alice, "http-client", "1.0.0", "a")).status, 201);
  const clash = await publish(call, bob, "http_client", "1.0.0", "b");
  assert.equal(clash.status, 403);
  assert.equal((await clash.json()).error.code, "E_NOT_OWNER");
  assert.equal((await publish(call, alice, "http_client", "1.1.0", "c")).status, 201);
});

test("bad requests are refused with a reason", async () => {
  const { call } = registry();
  const token = await newPublisher(call, "alice");
  for (const [name, version, status, code] of [
    ["regex", "1.0.0", 400, "E_NAME_INVALID"],
    ["Upper", "1.0.0", 400, "E_NAME_INVALID"],
    ["@acme%2Fx", "1.0.0", 400, "E_NAME_INVALID"],
    ["fine", "1.0", 400, "E_VERSION_INVALID"],
  ]) {
    const response = await publish(call, token, name, version, "x");
    assert.equal(response.status, status, name);
    assert.equal((await response.json()).error.code, code);
  }
  const body = new TextEncoder().encode("data");
  const wrongHash = await call("PUT", "v1/publish/fine/1.0.0", {
    token,
    body,
    headers: { "x-guion-sha256": "0".repeat(64) },
  });
  assert.equal((await wrongHash.json()).error.code, "E_HASH_MISMATCH");
  const anonymous = await call("PUT", "v1/publish/fine/1.0.0", { body });
  assert.equal(anonymous.status, 401);
  const forged = await call("PUT", "v1/publish/fine/1.0.0", { token: "gpt_forged", body });
  assert.equal(forged.status, 401);
  const notAdmin = await call("POST", "v1/admin/publishers", { token: "nope", body: JSON.stringify({ id: "x" }) });
  assert.equal(notAdmin.status, 401);
});

test("yank hides a version from the index flag but keeps the object, and is logged", async () => {
  const { call } = registry();
  const token = await newPublisher(call, "alice");
  const other = await newPublisher(call, "bob");
  const made = await (await publish(call, token, "sample", "1.0.0", "bytes")).json();
  assert.equal((await call("POST", "v1/yank/sample/1.0.0", { token: other })).status, 403);
  const yanked = await call("POST", "v1/yank/sample/1.0.0", { token });
  assert.equal(yanked.status, 200);
  const index = await (await call("GET", "index/sample.json")).json();
  assert.equal(index.versions[0].yanked, true);
  assert.equal((await call("GET", `objects/${made.sha256}.tar`)).status, 200);
  const entry = await (await call("GET", "log/0000000002.json")).json();
  assert.equal(entry.action, "yank");
  await call("POST", "v1/unyank/sample/1.0.0", { token });
  assert.equal((await (await call("GET", "index/sample.json")).json()).versions[0].yanked, false);
  assert.equal((await call("POST", "v1/yank/sample/9.9.9", { token })).status, 404);
});

test("private records are never served and tokens are stored only as hashes", async () => {
  const { env, call } = registry();
  const token = await newPublisher(call, "alice");
  await publish(call, token, "sample", "1.0.0", "x");
  for (const path of ["owners/sample.json", "tokens/anything.json", "versions/sample/1.0.0.json", "../owners/sample.json"]) {
    assert.equal((await call("GET", path)).status, 404, path);
  }
  for (const [key, entry] of env.REGISTRY.objects) {
    assert.ok(!new TextDecoder().decode(entry.bytes).includes(token), `token found in ${key}`);
  }
});

test("a failed publish can be retried and leaves no half-committed version", async () => {
  const { env, call } = registry();
  const token = await newPublisher(call, "alice");
  env.REGISTRY.failNextPutMatching = "log/0";
  const failed = await publish(call, token, "sample", "1.0.0", "payload");
  assert.equal(failed.status, 500);
  // The version is not visible: it is not in the index and the log has no entry.
  assert.equal((await call("GET", "index/sample.json")).status, 404);
  assert.equal((await call("GET", "log/head.json")).status, 404);
  // The publisher retries the same bytes and the publish completes.
  const retried = await publish(call, token, "sample", "1.0.0", "payload");
  assert.equal(retried.status, 201);
  const index = await (await call("GET", "index/sample.json")).json();
  assert.equal(index.versions.length, 1);
  assert.equal((await (await call("GET", "log/head.json")).json()).seq, 1);
  // Different bytes for a version that already completed are refused.
  assert.equal((await publish(call, token, "sample", "1.0.0", "other")).status, 409);
});

test("a publisher can rotate or revoke its own token, and the administrator can revoke them all", async () => {
  const { call } = registry();
  const first = await newPublisher(call, "alice");
  assert.equal((await publish(call, first, "rotating", "1.0.0", "one")).status, 201);

  const rotated = await call("POST", "v1/tokens/rotate", { token: first });
  assert.equal(rotated.status, 200);
  const second = (await rotated.json()).token;
  assert.match(second, /^gpt_[0-9a-f]{64}$/);
  assert.notEqual(second, first);
  assert.equal((await publish(call, first, "rotating", "1.1.0", "two")).status, 401);
  assert.equal((await publish(call, second, "rotating", "1.1.0", "two")).status, 201);

  const revoked = await call("POST", "v1/tokens/revoke", { token: second });
  assert.equal(revoked.status, 200);
  assert.equal((await revoked.json()).revoked, 1);
  assert.equal((await publish(call, second, "rotating", "1.2.0", "three")).status, 401);
  assert.equal((await call("POST", "v1/tokens/revoke", { token: second })).status, 401);

  // A second token for the same publisher, then an administrator revocation of both.
  const third = await newPublisher(call, "alice");
  const fourth = await newPublisher(call, "alice");
  const bob = await newPublisher(call, "bob");
  assert.equal((await call("POST", "v1/admin/publishers/alice/revoke", { token: third })).status, 401);
  const all = await call("POST", "v1/admin/publishers/alice/revoke", { token: ADMIN });
  assert.equal(all.status, 200);
  assert.equal((await all.json()).revoked, 2);
  for (const token of [third, fourth]) {
    assert.equal((await publish(call, token, "rotating", "1.3.0", "four")).status, 401);
  }
  // Other publishers are untouched, and token bookkeeping is never served.
  assert.equal((await publish(call, bob, "bobs-package", "1.0.0", "bob")).status, 201);
  assert.equal((await call("GET", "publisher-tokens/bob")).status, 404);
  assert.equal((await call("POST", "v1/admin/publishers/Not_Valid/revoke", { token: ADMIN })).status, 400);
});

test("administrator revocation also finds tokens issued before the per-publisher index", async () => {
  const { env, call } = registry();
  const legacy = await newPublisher(call, "carol");
  for (const key of [...env.REGISTRY.objects.keys()]) {
    if (key.startsWith("publisher-tokens/")) env.REGISTRY.objects.delete(key);
  }
  const revoked = await call("POST", "v1/admin/publishers/carol/revoke", { token: ADMIN });
  assert.equal((await revoked.json()).revoked, 1);
  assert.equal((await publish(call, legacy, "carols", "1.0.0", "c")).status, 401);
});
