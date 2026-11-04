//! Client side of the registry protocol (docs/registry-protocol.md): reading an index, verifying
//! that the index, the hash log and the downloaded bytes agree, publishing and yanking.
//!
//! This code only runs when a command asks for the network (`add NAME`, `publish`, `yank`); the
//! compiler and runtime never reach it.

use crate::package_version::{resolve, Candidate, PackageIndex, Version, VersionReq};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

fn parse_signature(text: &str) -> Result<[u8; 64], ()> {
    if text.len() != 128 || !text.is_ascii() {
        return Err(());
    }
    let mut signature = [0u8; 64];
    for (index, byte) in signature.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| ())?;
    }
    Ok(signature)
}

const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;

pub struct Registry {
    base: String,
    /// The ed25519 key every log entry must be signed with; `None` accepts unsigned entries.
    public_key: Option<[u8; 32]>,
}

/// The Guion package registry (registry/ deployed as a Cloudflare Worker over R2).
pub const DEFAULT_REGISTRY_URL: &str = "https://guion-registry.thomasbuild.workers.dev";
/// The default registry's log-signing key (ed25519, hex). Its key id is 2cbb71082c5a7848.
pub const DEFAULT_REGISTRY_PUBLIC_KEY: &str =
    "4b40803cbc49c33eb741d031ed339abd73139ae95ce502ef48a3d3051ac25b11";

fn parse_public_key(text: &str) -> Result<[u8; 32], String> {
    let text = text.trim();
    let invalid = || "a registry public key must be 64 hexadecimal characters".to_owned();
    if text.len() != 64 || !text.is_ascii() {
        return Err(invalid());
    }
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| invalid())?;
    }
    Ok(key)
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

impl Registry {
    /// The registry URL comes from `GUION_REGISTRY_URL`, then `[registry] url` in the project
    /// manifest, then the built-in default. The log-signing key comes from
    /// `GUION_REGISTRY_PUBLIC_KEY`, then `[registry] public_key`, then the key pinned for the
    /// default registry.
    pub fn configured(project: &Path) -> Result<Registry, String> {
        let from_manifest = |field: &str| -> Option<String> {
            let text = std::fs::read_to_string(project.join("guion.toml")).ok()?;
            let value: toml::Value = toml::from_str(&text).ok()?;
            value
                .get("registry")?
                .get(field)?
                .as_str()
                .map(str::to_owned)
        };
        let url = std::env::var("GUION_REGISTRY_URL")
            .ok()
            .or_else(|| from_manifest("url"))
            .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_owned());
        let mut registry = Registry::new(&url)?;
        if let Some(key) = std::env::var("GUION_REGISTRY_PUBLIC_KEY")
            .ok()
            .or_else(|| from_manifest("public_key"))
        {
            registry.public_key = Some(parse_public_key(&key)?);
        }
        Ok(registry)
    }

    pub fn new(url: &str) -> Result<Registry, String> {
        let parsed =
            reqwest::Url::parse(url).map_err(|error| format!("invalid registry URL: {error}"))?;
        let host = parsed.host_str().unwrap_or_default();
        let secure = parsed.scheme() == "https" || (parsed.scheme() == "http" && is_loopback(host));
        if !secure
            || host.is_empty()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
        {
            return Err("the registry URL must use https (http is allowed only for localhost) and cannot contain credentials".into());
        }
        let base = url.trim_end_matches('/').to_owned();
        let public_key = (base == DEFAULT_REGISTRY_URL)
            .then(|| parse_public_key(DEFAULT_REGISTRY_PUBLIC_KEY))
            .transpose()?;
        Ok(Registry { base, public_key })
    }

    pub fn object_url(&self, sha256: &str) -> String {
        format!("{}/objects/{sha256}.tar", self.base)
    }

    fn client() -> Result<(reqwest::Client, tokio::runtime::Runtime), String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|error| format!("cannot configure the registry client: {error}"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("cannot start the registry client: {error}"))?;
        Ok((client, runtime))
    }

    fn get(&self, path: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
        let (client, runtime) = Self::client()?;
        let url = format!("{}/{path}", self.base);
        runtime.block_on(async move {
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|error| format!("cannot reach the registry: {error}"))?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !response.status().is_success() {
                return Err(format!("the registry answered HTTP {}", response.status()));
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|error| format!("cannot read the registry response: {error}"))?;
            if bytes.len() > limit {
                return Err("the registry response is larger than the allowed limit".into());
            }
            Ok(Some(bytes.to_vec()))
        })
    }

    fn get_json(&self, path: &str) -> Result<Option<Value>, String> {
        match self.get(path, MAX_INDEX_BYTES)? {
            None => Ok(None),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| format!("the registry returned invalid JSON for {path}: {error}")),
        }
    }

    fn index(&self, name: &str) -> Result<Option<Value>, String> {
        self.get_json(&format!(
            "index/{}.json",
            crate::package_name::collision_key(name)
        ))
    }

    /// Resolves `requirement` for `name` against the registry and returns the chosen version's
    /// index entry, after checking it against the hash log.
    pub fn select(
        &self,
        name: &str,
        requirement: &VersionReq,
        pinned: &BTreeMap<String, Version>,
    ) -> Result<VerifiedVersion, String> {
        let source = RegistryIndex { registry: self };
        let chosen = resolve(&[(name.to_owned(), requirement.clone())], &source, pinned)?;
        if chosen.len() > 1 {
            let others: Vec<&str> = chosen
                .keys()
                .map(String::as_str)
                .filter(|other| *other != name)
                .collect();
            return Err(format!(
                "`{name}` depends on other registry packages ({}); installing registry dependencies is not supported yet",
                others.join(", ")
            ));
        }
        let version = chosen[name];
        let index = self
            .index(name)?
            .ok_or_else(|| format!("the registry has no package named `{name}`"))?;
        let entry = index["versions"]
            .as_array()
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|entry| entry["version"].as_str() == Some(&version.to_string()))
            })
            .ok_or_else(|| format!("{name} {version} vanished from the registry index"))?;
        let sha256 = entry["sha256"]
            .as_str()
            .ok_or("the registry index entry has no sha256")?
            .to_ascii_lowercase();
        let log_seq = entry["log_seq"]
            .as_u64()
            .ok_or("the registry index entry has no log_seq")?;
        self.verify_log(
            &crate::package_name::collision_key(name),
            &version.to_string(),
            &sha256,
            log_seq,
        )?;
        Ok(VerifiedVersion {
            version: version.to_string(),
            sha256,
        })
    }

    /// The log entry named by the index must carry the same hash, and must itself be intact and
    /// chained to the entry before it.
    fn verify_log(&self, name: &str, version: &str, sha256: &str, seq: u64) -> Result<(), String> {
        let entry = self.log_entry(seq)?;
        let matches = entry["action"] == "publish"
            && entry["name"] == name
            && entry["version"] == version
            && entry["sha256"] == sha256;
        if !matches {
            return Err(format!(
                "the registry log entry {seq} does not match the index for {name} {version}; refusing to install"
            ));
        }
        let previous = if seq > 1 {
            self.log_entry(seq - 1)?["hash"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        } else {
            "0".repeat(64)
        };
        if entry["prev"] != previous.as_str() {
            return Err(format!("the registry log is not chained at entry {seq}"));
        }
        Ok(())
    }

    fn log_entry(&self, seq: u64) -> Result<Value, String> {
        let entry = self
            .get_json(&format!("log/{seq:010}.json"))?
            .ok_or_else(|| format!("the registry log has no entry {seq}"))?;
        let object = entry
            .as_object()
            .ok_or("a registry log entry is not an object")?;
        // The registry hashes the entry's JSON text with its keys in this fixed order, so the text
        // is rebuilt by hand rather than through a map whose key order may differ.
        let mut text = String::from("{");
        for (position, key) in [
            "seq",
            "time",
            "action",
            "name",
            "version",
            "sha256",
            "publisher",
            "prev",
        ]
        .iter()
        .enumerate()
        {
            if position > 0 {
                text.push(',');
            }
            let value = object.get(*key).cloned().unwrap_or(Value::Null);
            text.push_str(&format!("\"{key}\":{value}"));
        }
        text.push('}');
        let expected = format!("{:x}", Sha256::digest(text.as_bytes()));
        if entry["hash"] != expected.as_str() {
            return Err(format!("registry log entry {seq} fails its own hash check"));
        }
        if let Some(key) = &self.public_key {
            // The signature covers the hash, which covers `prev`: a valid signature on an entry
            // vouches for every entry before it.
            let signature = entry["signature"]
                .as_str()
                .and_then(|text| parse_signature(text).ok());
            let message = format!("guion-registry-log-v1:{seq}:{expected}");
            let valid = signature.is_some_and(|signature| {
                ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
                    .verify(message.as_bytes(), &signature)
                    .is_ok()
            });
            if !valid {
                return Err(format!(
                    "registry log entry {seq} is not signed by the registry's key; refusing to install"
                ));
            }
        }
        Ok(entry)
    }

    #[cfg(test)]
    fn with_public_key(mut self, key: [u8; 32]) -> Registry {
        self.public_key = Some(key);
        self
    }

    pub fn download(&self, sha256: &str) -> Result<Vec<u8>, String> {
        let bytes = self
            .get(&format!("objects/{sha256}.tar"), MAX_ARCHIVE_BYTES)?
            .ok_or("the registry has no archive for that version")?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != sha256 {
            return Err(format!(
                "the downloaded archive hash {actual} does not match the registry's {sha256}"
            ));
        }
        Ok(bytes)
    }

    fn token() -> Result<String, String> {
        std::env::var("GUION_REGISTRY_TOKEN")
            .ok()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| "set GUION_REGISTRY_TOKEN to your publisher token".to_owned())
    }

    /// Error text from the network layer never contains the token, but strip it anyway.
    fn redact(message: String, token: &str) -> String {
        message.replace(token, "[token]")
    }

    pub fn publish(
        &self,
        name: &str,
        version: &str,
        archive: Vec<u8>,
        meta: Value,
    ) -> Result<String, String> {
        let token = Self::token()?;
        let sha256 = format!("{:x}", Sha256::digest(&archive));
        let (client, runtime) = Self::client()?;
        let url = format!("{}/v1/publish/{name}/{version}", self.base);
        let result = runtime.block_on(async {
            let response = client
                .put(&url)
                .bearer_auth(&token)
                .header("x-guion-sha256", &sha256)
                .header("x-guion-meta", meta.to_string())
                .body(archive)
                .send()
                .await
                .map_err(|error| format!("cannot reach the registry: {error}"))?;
            Self::read_outcome(response).await
        });
        let body = result.map_err(|message| Self::redact(message, &token))?;
        Ok(format!(
            "published {name} {version} ({sha256}), log entry {}",
            body["log_seq"]
        ))
    }

    pub fn set_yanked(&self, name: &str, version: &str, yanked: bool) -> Result<String, String> {
        let token = Self::token()?;
        let (client, runtime) = Self::client()?;
        let action = if yanked { "yank" } else { "unyank" };
        let url = format!(
            "{}/v1/{action}/{}/{version}",
            self.base,
            crate::package_name::collision_key(name)
        );
        let result = runtime.block_on(async {
            let response = client
                .post(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| format!("cannot reach the registry: {error}"))?;
            Self::read_outcome(response).await
        });
        result.map_err(|message| Self::redact(message, &token))?;
        Ok(format!("{action}ed {name} {version}"))
    }

    /// Retires the token in `GUION_REGISTRY_TOKEN`. With `rotate`, the registry first issues a
    /// replacement, which is returned; without it the token is revoked outright.
    pub fn retire_token(&self, rotate: bool) -> Result<String, String> {
        let token = Self::token()?;
        let (client, runtime) = Self::client()?;
        let action = if rotate { "rotate" } else { "revoke" };
        let url = format!("{}/v1/tokens/{action}", self.base);
        let result = runtime.block_on(async {
            let response = client
                .post(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| format!("cannot reach the registry: {error}"))?;
            Self::read_outcome(response).await
        });
        let body = result.map_err(|message| Self::redact(message, &token))?;
        let publisher = body["publisher"].as_str().unwrap_or("the publisher");
        if !rotate {
            return Ok(format!(
                "revoked the token for {publisher}; it no longer works"
            ));
        }
        let replacement = body["token"]
            .as_str()
            .ok_or("the registry did not return a replacement token")?;
        Ok(format!(
            "new token for {publisher} (the previous one no longer works; store this now, it is not shown again):\n{replacement}"
        ))
    }

    async fn read_outcome(response: reqwest::Response) -> Result<Value, String> {
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| format!("cannot read the registry response: {error}"))?;
        let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(body);
        }
        let code = body["error"]["code"].as_str().unwrap_or("E_REGISTRY");
        let message = body["error"]["message"]
            .as_str()
            .unwrap_or("the registry refused the request");
        Err(format!("{code}: {message} (HTTP {status})"))
    }
}

pub struct VerifiedVersion {
    pub version: String,
    pub sha256: String,
}

struct RegistryIndex<'a> {
    registry: &'a Registry,
}

impl PackageIndex for RegistryIndex<'_> {
    fn candidates(&self, name: &str) -> Result<Vec<Candidate>, String> {
        let Some(index) = self.registry.index(name)? else {
            return Ok(Vec::new());
        };
        let mut candidates = Vec::new();
        for entry in index["versions"].as_array().into_iter().flatten() {
            let version = Version::parse(entry["version"].as_str().unwrap_or_default())?;
            let mut dependencies = Vec::new();
            if let Some(map) = entry["dependencies"].as_object() {
                for (dependency, requirement) in map {
                    dependencies.push((
                        dependency.clone(),
                        VersionReq::parse(requirement.as_str().unwrap_or("*"))?,
                    ));
                }
            }
            candidates.push(Candidate {
                version,
                dependencies,
                yanked: entry["yanked"].as_bool().unwrap_or(false),
            });
        }
        Ok(candidates)
    }
}

/// An uncompressed ustar archive of `root`, in a fixed order with fixed metadata, so the same
/// source always produces the same bytes (and therefore the same hash).
pub fn tar_directory(root: &Path) -> Result<Vec<u8>, String> {
    let mut entries = Vec::new();
    collect(root, root, &mut entries)?;
    entries.sort();
    let mut out = Vec::new();
    for relative in entries {
        let full = root.join(&relative);
        let is_dir = full.is_dir();
        let data = if is_dir {
            Vec::new()
        } else {
            std::fs::read(&full)
                .map_err(|error| format!("cannot read {}: {error}", full.display()))?
        };
        let name = if is_dir {
            format!("{relative}/")
        } else {
            relative.clone()
        };
        write_header(&mut out, &name, data.len() as u64, is_dir)?;
        out.extend_from_slice(&data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.extend_from_slice(&[0u8; 1024]);
    Ok(out)
}

fn collect(root: &Path, directory: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot read a directory entry: {error}"))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "path escaped the package root".to_owned())?
            .to_string_lossy()
            .replace('\\', "/");
        if kind.is_symlink() {
            return Err(format!(
                "{relative} is a symbolic link; packages cannot contain links"
            ));
        }
        if kind.is_dir() {
            out.push(relative);
            collect(root, &path, out)?;
        } else if kind.is_file() {
            out.push(relative);
        }
    }
    Ok(())
}

fn write_header(out: &mut Vec<u8>, name: &str, size: u64, is_dir: bool) -> Result<(), String> {
    let (prefix, tail) = if name.len() <= 100 {
        ("", name)
    } else {
        let split = name[..name.len().min(156)]
            .rfind('/')
            .filter(|index| name.len() - index - 1 <= 100)
            .ok_or_else(|| format!("path is too long for a package archive: {name}"))?;
        (&name[..split], &name[split + 1..])
    };
    let mut header = [0u8; 512];
    header[..tail.len()].copy_from_slice(tail.as_bytes());
    let mode: &[u8] = if is_dir { b"0000755" } else { b"0000644" };
    header[100..107].copy_from_slice(mode);
    header[108..115].copy_from_slice(b"0000000");
    header[116..123].copy_from_slice(b"0000000");
    header[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
    header[136..147].copy_from_slice(b"00000000000");
    header[148..156].copy_from_slice(b"        ");
    header[156] = if is_dir { b'5' } else { b'0' };
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    header[148..155].copy_from_slice(format!("{checksum:06o}\0").as_bytes());
    header[155] = b' ';
    out.extend_from_slice(&header);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_urls_must_be_https_or_loopback() {
        assert!(Registry::new("https://registry.example/").is_ok());
        assert!(Registry::new("http://127.0.0.1:8787").is_ok());
        assert!(Registry::new("http://localhost:8787").is_ok());
        assert!(Registry::new("http://registry.example").is_err());
        assert!(Registry::new("https://user:pw@registry.example").is_err());
        assert!(Registry::new("ftp://registry.example").is_err());
    }

    #[test]
    fn tar_output_is_deterministic_and_extractable() {
        let dir = std::env::temp_dir().join(format!("guion-tar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/deep")).unwrap();
        std::fs::write(dir.join("guion.toml"), "[package]\n").unwrap();
        std::fs::write(
            dir.join("src/deep/lib.gu"),
            "pub fn x() -> Int:\n    return 1\n",
        )
        .unwrap();
        let first = tar_directory(&dir).unwrap();
        let second = tar_directory(&dir).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len() % 512, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A one-thread HTTP server that answers each path from a fixed table.
    fn serve(routes: Vec<(&'static str, String)>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buffer = [0u8; 2048];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let body = routes
                    .iter()
                    .find(|(route, _)| *route == path)
                    .map(|(_, body)| body.clone());
                let response = match body {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_owned()
                    }
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{address}")
    }

    fn log_entry_json(version: &str, sha: &str, forged_hash: Option<&str>) -> String {
        let text = format!(
            "{{\"seq\":1,\"time\":\"2026-01-01T00:00:00.000Z\",\"action\":\"publish\",\"name\":\"demo\",\"version\":\"{version}\",\"sha256\":\"{sha}\",\"publisher\":\"alice\",\"prev\":\"{}\"}}",
            "0".repeat(64)
        );
        let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
        let hash = forged_hash.map_or(hash, str::to_owned);
        format!("{},\"hash\":\"{hash}\"}}", &text[..text.len() - 1])
    }

    /// A throwaway signing key for these tests; it protects nothing.
    fn test_key(seed: u8) -> ring::signature::Ed25519KeyPair {
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
    }

    fn public_key(key: &ring::signature::Ed25519KeyPair) -> [u8; 32] {
        use ring::signature::KeyPair;
        key.public_key().as_ref().try_into().unwrap()
    }

    fn signed_log_entry_json(version: &str, sha: &str, key: &ring::signature::Ed25519KeyPair) -> String {
        let entry = log_entry_json(version, sha, None);
        let value: Value = serde_json::from_str(&entry).unwrap();
        let hash = value["hash"].as_str().unwrap();
        let signature = key.sign(format!("guion-registry-log-v1:1:{hash}").as_bytes());
        let hex: String = signature.as_ref().iter().map(|byte| format!("{byte:02x}")).collect();
        format!("{},\"key_id\":\"test\",\"signature\":\"{hex}\"}}", &entry[..entry.len() - 1])
    }

    fn select_demo(registry: Registry) -> Result<VerifiedVersion, String> {
        registry.select("demo", &VersionReq::parse("*").unwrap(), &BTreeMap::new())
    }

    #[test]
    fn the_default_registry_has_its_signing_key_pinned() {
        let registry = Registry::new(DEFAULT_REGISTRY_URL).unwrap();
        assert_eq!(
            registry.public_key,
            Some(parse_public_key(DEFAULT_REGISTRY_PUBLIC_KEY).unwrap())
        );
        assert!(Registry::new("https://registry.example").unwrap().public_key.is_none());
        assert!(parse_public_key("abc").is_err());
    }

    #[test]
    fn a_log_entry_signed_by_the_registry_key_verifies() {
        let key = test_key(7);
        let sha = "a".repeat(64);
        let base = serve(vec![
            ("/index/demo.json", index_json(&sha)),
            ("/log/0000000001.json", signed_log_entry_json("1.0.0", &sha, &key)),
        ]);
        let registry = Registry::new(&base).unwrap().with_public_key(public_key(&key));
        assert_eq!(select_demo(registry).unwrap().sha256, sha);
    }

    #[test]
    fn unsigned_or_wrongly_signed_log_entries_fail_closed_when_a_key_is_known() {
        let key = test_key(7);
        let sha = "a".repeat(64);
        for entry in [
            log_entry_json("1.0.0", &sha, None),
            signed_log_entry_json("1.0.0", &sha, &test_key(8)),
        ] {
            let base = serve(vec![
                ("/index/demo.json", index_json(&sha)),
                ("/log/0000000001.json", entry),
            ]);
            let registry = Registry::new(&base).unwrap().with_public_key(public_key(&key));
            let error = select_demo(registry).err().expect("must refuse");
            assert!(error.contains("not signed by the registry's key"), "{error}");
        }
    }

    fn index_json(sha: &str) -> String {
        format!(
            "{{\"schema\":1,\"name\":\"demo\",\"owner\":\"alice\",\"versions\":[{{\"version\":\"1.0.0\",\"sha256\":\"{sha}\",\"size\":5,\"yanked\":false,\"log_seq\":1,\"dependencies\":{{}},\"signature\":null}}]}}"
        )
    }

    #[test]
    fn a_consistent_index_log_and_object_verify() {
        let archive = "bytes";
        let sha = format!("{:x}", Sha256::digest(archive.as_bytes()));
        let base = serve(vec![
            ("/index/demo.json", index_json(&sha)),
            ("/log/0000000001.json", log_entry_json("1.0.0", &sha, None)),
        ]);
        let registry = Registry::new(&base).unwrap();
        let chosen = registry
            .select(
                "demo",
                &VersionReq::parse("^1.0").unwrap(),
                &BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(
            (chosen.version.as_str(), chosen.sha256.as_str()),
            ("1.0.0", sha.as_str())
        );
    }

    #[test]
    fn a_log_that_disagrees_with_the_index_fails_closed() {
        let sha = "a".repeat(64);
        let other = "b".repeat(64);
        let base = serve(vec![
            ("/index/demo.json", index_json(&sha)),
            (
                "/log/0000000001.json",
                log_entry_json("1.0.0", &other, None),
            ),
        ]);
        let error = Registry::new(&base)
            .unwrap()
            .select("demo", &VersionReq::parse("*").unwrap(), &BTreeMap::new())
            .err()
            .expect("must refuse");
        assert!(error.contains("does not match the index"), "{error}");
    }

    #[test]
    fn a_tampered_log_entry_fails_its_own_hash() {
        let sha = "a".repeat(64);
        let base = serve(vec![
            ("/index/demo.json", index_json(&sha)),
            (
                "/log/0000000001.json",
                log_entry_json("1.0.0", &sha, Some(&"f".repeat(64))),
            ),
        ]);
        let error = Registry::new(&base)
            .unwrap()
            .select("demo", &VersionReq::parse("*").unwrap(), &BTreeMap::new())
            .err()
            .expect("must refuse");
        assert!(error.contains("fails its own hash check"), "{error}");
    }

    #[test]
    fn an_object_whose_bytes_do_not_match_its_name_is_refused() {
        let claimed = "c".repeat(64);
        let base = serve(vec![(
            Box::leak(format!("/objects/{claimed}.tar").into_boxed_str()),
            "not the claimed content".to_owned(),
        )]);
        let error = Registry::new(&base)
            .unwrap()
            .download(&claimed)
            .expect_err("must refuse");
        assert!(error.contains("does not match"), "{error}");
    }
}
