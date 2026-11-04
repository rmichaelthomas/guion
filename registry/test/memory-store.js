// A small in-memory stand-in for an R2 bucket, enough for the registry core.
export class MemoryStore {
  constructor() {
    this.objects = new Map();
    this.failNextPutMatching = null;
  }

  async get(key) {
    const entry = this.objects.get(key);
    if (!entry) return null;
    const bytes = entry.bytes;
    return {
      etag: entry.etag,
      body: new Blob([bytes]).stream(),
      text: async () => new TextDecoder().decode(bytes),
      arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
    };
  }

  async put(key, value, options = {}) {
    if (this.failNextPutMatching && key.startsWith(this.failNextPutMatching)) {
      this.failNextPutMatching = null;
      throw new Error("simulated storage failure");
    }
    if (options.onlyIf?.etagDoesNotMatch === "*" && this.objects.has(key)) return null;
    const bytes = typeof value === "string" ? new TextEncoder().encode(value) : new Uint8Array(value);
    const etag = String(this.objects.size + Math.random());
    this.objects.set(key, { bytes, etag });
    return { key, etag };
  }

  async list({ prefix = "" } = {}) {
    const objects = [...this.objects.keys()].filter((key) => key.startsWith(prefix)).sort().map((key) => ({ key }));
    return { objects, truncated: false };
  }
}
