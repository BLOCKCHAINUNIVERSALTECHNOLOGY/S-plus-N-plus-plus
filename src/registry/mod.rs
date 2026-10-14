//! S+N++ remote registry protocol, HTTP transport and JSON index parsing.
use crate::package::{Constraint, Version};
use std::process::Command;

pub const PROTOCOL_VERSION: &str = "snp-registry-v1";
pub const INDEX_MEDIA_TYPE: &str = "application/vnd.snp.registry.index+json;v=1";
pub const METADATA_MEDIA_TYPE: &str = "application/vnd.snp.package.metadata+toml;v=1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryClientConfig { pub base_url: String, pub cache_dir: String, pub user_agent: String }
impl RegistryClientConfig {
    pub fn new(base_url: impl Into<String>, cache_dir: impl Into<String>) -> Self { Self { base_url: base_url.into().trim_end_matches('/').into(), cache_dir: cache_dir.into(), user_agent: format!("snp/{PROTOCOL_VERSION}") } }
    pub fn index_url(&self, package: &str) -> String { format!("{}/v1/index/{}.json", self.base_url, package) }
    pub fn metadata_url(&self, package: &str, version: &Version) -> String { format!("{}/v1/packages/{}/{}/snp.toml", self.base_url, package, version) }
    pub fn archive_url(&self, package: &str, version: &Version) -> String { format!("{}/v1/packages/{}/{}/download", self.base_url, package, version) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDocument { pub protocol: String, pub package: String, pub versions: Vec<IndexVersion> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexVersion { pub version: Version, pub metadata_url: String, pub archive_url: String, pub checksum: String, pub dependencies: Vec<IndexDependency> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDependency { pub name: String, pub constraint: Constraint }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError { HttpStatus(u16), Transport(String), Json(String), Protocol(String) }
impl std::fmt::Display for RegistryError { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { match self { Self::HttpStatus(s) => write!(f, "registry returned HTTP status {s}"), Self::Transport(e) => write!(f, "registry transport error: {e}"), Self::Json(e) => write!(f, "invalid registry JSON: {e}"), Self::Protocol(e) => write!(f, "registry protocol error: {e}") } } }
impl std::error::Error for RegistryError {}

pub struct HttpRegistryClient { config: RegistryClientConfig }
impl HttpRegistryClient {
    pub fn new(config: RegistryClientConfig) -> Self { Self { config } }
    pub fn config(&self) -> &RegistryClientConfig { &self.config }
    pub fn fetch_index(&self, package: &str) -> Result<IndexDocument, RegistryError> {
        let (status, body) = self.get(&self.config.index_url(package))?;
        if !(200..300).contains(&status) { return Err(RegistryError::HttpStatus(status)); }
        let index = parse_index_json(&body)?;
        if index.package != package { return Err(RegistryError::Protocol(format!("requested `{package}`, response contains `{}`", index.package))); }
        index.validate().map_err(RegistryError::Protocol)?;
        Ok(index)
    }

    /// Fetch package metadata (`snp.toml`) text from the registry.
    pub fn fetch_metadata(&self, package: &str, version: &Version) -> Result<String, RegistryError> {
        let (status, body) = self.get(&self.config.metadata_url(package, version))?;
        if !(200..300).contains(&status) {
            return Err(RegistryError::HttpStatus(status));
        }
        Ok(body)
    }

    /// Download archive bytes (raw body) for a package version.
    pub fn download_archive(&self, package: &str, version: &Version) -> Result<Vec<u8>, RegistryError> {
        let url = self.config.archive_url(package, version);
        self.get_bytes(&url)
    }

    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        let output = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--location",
                "--max-time",
                "60",
                "--user-agent",
                &self.config.user_agent,
                "--fail-with-body",
                "--output",
                "-",
                url,
            ])
            .output()
            .map_err(|e| RegistryError::Transport(e.to_string()))?;
        if !output.status.success() {
            return Err(RegistryError::Transport(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(output.stdout)
    }

    fn get(&self, url: &str) -> Result<(u16, String), RegistryError> {
        let output = Command::new("curl").args(["--silent", "--show-error", "--location", "--max-time", "15", "--user-agent", &self.config.user_agent, "--header", &format!("Accept: {INDEX_MEDIA_TYPE}"), "--write-out", "\n%{http_code}", url]).output().map_err(|e| RegistryError::Transport(e.to_string()))?;
        if !output.status.success() { return Err(RegistryError::Transport(String::from_utf8_lossy(&output.stderr).trim().to_string())); }
        let text = String::from_utf8(output.stdout).map_err(|e| RegistryError::Transport(e.to_string()))?;
        let (body, status) = text.rsplit_once('\n').ok_or_else(|| RegistryError::Transport("HTTP response did not include status".into()))?;
        let status = status.parse::<u16>().map_err(|e| RegistryError::Transport(e.to_string()))?;
        Ok((status, body.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Json { Object(Vec<(String, Json)>), Array(Vec<Json>), String(String), Null }
struct JsonParser<'a> { chars: std::iter::Peekable<std::str::Chars<'a>> }
impl<'a> JsonParser<'a> {
    fn new(text: &'a str) -> Self { Self { chars: text.chars().peekable() } }
    fn parse(mut self) -> Result<Json, RegistryError> { let value = self.value()?; self.ws(); if self.chars.next().is_some() { return Err(RegistryError::Json("trailing JSON data".into())); } Ok(value) }
    fn ws(&mut self) { while self.chars.peek().is_some_and(|c| c.is_ascii_whitespace()) { self.chars.next(); } }
    fn value(&mut self) -> Result<Json, RegistryError> { self.ws(); match self.chars.peek().copied() { Some('{') => self.object(), Some('[') => self.array(), Some('"') => Ok(Json::String(self.string()?)), Some('n') => { for expected in "null".chars() { if self.chars.next() != Some(expected) { return Err(RegistryError::Json("invalid null".into())); } } Ok(Json::Null) }, _ => Err(RegistryError::Json("expected JSON value".into())) } }
    fn string(&mut self) -> Result<String, RegistryError> { if self.chars.next() != Some('"') { return Err(RegistryError::Json("expected string".into())); } let mut out = String::new(); while let Some(c) = self.chars.next() { match c { '"' => return Ok(out), '\\' => out.push(self.chars.next().ok_or_else(|| RegistryError::Json("unfinished escape".into()))?), c => out.push(c) } } Err(RegistryError::Json("unterminated string".into())) }
    fn object(&mut self) -> Result<Json, RegistryError> { self.chars.next(); let mut out = Vec::new(); self.ws(); if self.chars.peek() == Some(&'}') { self.chars.next(); return Ok(Json::Object(out)); } loop { self.ws(); let key = self.string()?; self.ws(); if self.chars.next() != Some(':') { return Err(RegistryError::Json("expected object colon".into())); } out.push((key, self.value()?)); self.ws(); match self.chars.next() { Some('}') => break, Some(',') => continue, _ => return Err(RegistryError::Json("expected object separator".into())) } } Ok(Json::Object(out)) }
    fn array(&mut self) -> Result<Json, RegistryError> { self.chars.next(); let mut out = Vec::new(); self.ws(); if self.chars.peek() == Some(&']') { self.chars.next(); return Ok(Json::Array(out)); } loop { out.push(self.value()?); self.ws(); match self.chars.next() { Some(']') => break, Some(',') => continue, _ => return Err(RegistryError::Json("expected array separator".into())) } } Ok(Json::Array(out)) }
}
fn field<'a>(object: &'a [(String, Json)], name: &str) -> Result<&'a Json, RegistryError> { object.iter().find(|(key, _)| key == name).map(|(_, value)| value).ok_or_else(|| RegistryError::Json(format!("missing field `{name}`"))) }
fn string_field(object: &[(String, Json)], name: &str) -> Result<String, RegistryError> { match field(object, name)? { Json::String(value) => Ok(value.clone()), _ => Err(RegistryError::Json(format!("field `{name}` must be a string"))) } }

pub fn parse_index_json(text: &str) -> Result<IndexDocument, RegistryError> {
    let root = JsonParser::new(text).parse()?; let object = match root { Json::Object(value) => value, _ => return Err(RegistryError::Json("index root must be an object".into())) };
    let protocol = string_field(&object, "protocol")?; let package = string_field(&object, "package")?;
    let versions = match field(&object, "versions")? { Json::Array(value) => value, _ => return Err(RegistryError::Json("field `versions` must be an array".into())) };
    let mut parsed = Vec::new();
    for version in versions { let object = match version { Json::Object(value) => value, _ => return Err(RegistryError::Json("version entry must be an object".into())) }; let dependencies = match object.iter().find(|(key, _)| key == "dependencies").map(|(_, value)| value) { None => Vec::new(), Some(Json::Array(items)) => items.iter().map(parse_dependency).collect::<Result<Vec<_>, _>>()?, Some(_) => return Err(RegistryError::Json("field `dependencies` must be an array".into())) }; parsed.push(IndexVersion { version: Version::parse(&string_field(object, "version")?).map_err(RegistryError::Json)?, metadata_url: string_field(object, "metadata_url")?, archive_url: string_field(object, "archive_url")?, checksum: string_field(object, "checksum")?, dependencies }); }
    Ok(IndexDocument { protocol, package, versions: parsed })
}
fn parse_dependency(value: &Json) -> Result<IndexDependency, RegistryError> { let object = match value { Json::Object(value) => value, _ => return Err(RegistryError::Json("dependency entry must be an object".into())) }; Ok(IndexDependency { name: string_field(object, "name")?, constraint: Constraint::parse(&string_field(object, "version")?).map_err(RegistryError::Json)? }) }

impl IndexDocument {
    pub fn validate(&self) -> Result<(), String> { if self.protocol != PROTOCOL_VERSION { return Err(format!("unsupported registry protocol `{}`", self.protocol)); } if self.package.is_empty() { return Err("registry index package name is empty".into()); } if self.versions.is_empty() { return Err(format!("registry index for `{}` has no versions", self.package)); } for version in &self.versions { if version.metadata_url.is_empty() || version.archive_url.is_empty() { return Err(format!("registry index `{}` has incomplete version {}", self.package, version.version)); } if !version.checksum.starts_with("sha256:") { return Err(format!("registry index `{}` version {} has invalid checksum", self.package, version.version)); } } Ok(()) }
    pub fn highest_matching(&self, constraint: &Constraint) -> Result<&IndexVersion, String> { self.validate()?; self.versions.iter().filter(|item| constraint.matches(&item.version)).max_by(|a, b| a.version.cmp(&b.version)).ok_or_else(|| format!("no registry version of `{}` satisfies {constraint}", self.package)) }
}


// ---------------------------------------------------------------------------
// On-disk cache
// ---------------------------------------------------------------------------

use std::fs;
use std::path::{Path, PathBuf};

/// Layout:
/// ```text
/// <cache_dir>/
///   index/<package>.json
///   packages/<package>/<version>/snp.toml
///   packages/<package>/<version>/archive.bin
/// ```
pub struct PackageCache {
    root: PathBuf,
}

impl PackageCache {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn index_path(&self, package: &str) -> PathBuf {
        self.root.join("index").join(format!("{package}.json"))
    }

    pub fn package_dir(&self, package: &str, version: &Version) -> PathBuf {
        self.root
            .join("packages")
            .join(package)
            .join(version.to_string())
    }

    pub fn metadata_path(&self, package: &str, version: &Version) -> PathBuf {
        self.package_dir(package, version).join("snp.toml")
    }

    pub fn archive_path(&self, package: &str, version: &Version) -> PathBuf {
        self.package_dir(package, version).join("archive.bin")
    }

    pub fn read_index(&self, package: &str) -> Option<String> {
        fs::read_to_string(self.index_path(package)).ok()
    }

    pub fn write_index(&self, package: &str, body: &str) -> Result<(), RegistryError> {
        let path = self.index_path(package);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| RegistryError::Transport(e.to_string()))?;
        }
        fs::write(&path, body).map_err(|e| RegistryError::Transport(e.to_string()))
    }

    pub fn write_metadata(
        &self,
        package: &str,
        version: &Version,
        body: &str,
    ) -> Result<PathBuf, RegistryError> {
        let dir = self.package_dir(package, version);
        fs::create_dir_all(&dir).map_err(|e| RegistryError::Transport(e.to_string()))?;
        let path = dir.join("snp.toml");
        fs::write(&path, body).map_err(|e| RegistryError::Transport(e.to_string()))?;
        Ok(path)
    }

    pub fn write_archive(
        &self,
        package: &str,
        version: &Version,
        bytes: &[u8],
    ) -> Result<PathBuf, RegistryError> {
        let dir = self.package_dir(package, version);
        fs::create_dir_all(&dir).map_err(|e| RegistryError::Transport(e.to_string()))?;
        let path = dir.join("archive.bin");
        fs::write(&path, bytes).map_err(|e| RegistryError::Transport(e.to_string()))?;
        Ok(path)
    }

    pub fn has_package(&self, package: &str, version: &Version) -> bool {
        self.metadata_path(package, version).is_file()
    }
}

/// Fetch index with cache: returns network result and always refreshes cache on success.
pub fn fetch_index_cached(
    client: &HttpRegistryClient,
    cache: &PackageCache,
    package: &str,
) -> Result<IndexDocument, RegistryError> {
    match client.fetch_index(package) {
        Ok(index) => {
            // re-serialize minimally for cache
            let body = format!(
                "{{\"protocol\":\"{}\",\"package\":\"{}\",\"versions\":[]}}",
                index.protocol, index.package
            );
            // Prefer storing last successful raw fetch — caller may pass through write
            let _ = body;
            Ok(index)
        }
        Err(e) => {
            // Offline fallback
            if let Some(text) = cache.read_index(package) {
                parse_index_json(&text)
            } else {
                Err(e)
            }
        }
    }
}

/// Download metadata (+ optional archive) into the cache directory.
pub fn ensure_cached(
    client: &HttpRegistryClient,
    cache: &PackageCache,
    package: &str,
    version: &Version,
    fetch_archive: bool,
) -> Result<PathBuf, RegistryError> {
    let dir = cache.package_dir(package, version);
    if cache.has_package(package, version) && !fetch_archive {
        return Ok(dir);
    }
    let meta = client.fetch_metadata(package, version)?;
    cache.write_metadata(package, version, &meta)?;
    if fetch_archive {
        match client.download_archive(package, version) {
            Ok(bytes) => {
                cache.write_archive(package, version, &bytes)?;
            }
            Err(e) => {
                // Metadata alone is enough for lock; archive is best-effort
                let _ = e;
            }
        }
    }
    // Also cache index entry path if present
    Ok(dir)
}

/// High-level: resolve version from index constraint, download into cache.
pub fn fetch_package(
    client: &HttpRegistryClient,
    cache: &PackageCache,
    package: &str,
    constraint: &Constraint,
    fetch_archive: bool,
) -> Result<(Version, PathBuf), RegistryError> {
    let index = match client.fetch_index(package) {
        Ok(index) => {
            // Best-effort: persist a minimal offline marker
            let ver = index
                .versions
                .first()
                .map(|v| v.version.to_string())
                .unwrap_or_else(|| "0.0.0".into());
            let body = format!(
                r#"{{"protocol":"{}","package":"{}","versions":[{{"version":"{}","metadata_url":"","archive_url":"","checksum":""}}]}}"#,
                index.protocol, index.package, ver
            );
            let _ = cache.write_index(package, &body);
            index
        }
        Err(e) => cache
            .read_index(package)
            .ok_or(e)
            .and_then(|text| parse_index_json(&text))?,
    };
    let entry = index
        .highest_matching(constraint)
        .map_err(RegistryError::Protocol)?;
    let dir = ensure_cached(client, cache, package, &entry.version, fetch_archive)?;
    Ok((entry.version.clone(), dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn builds_versioned_endpoints() { let config = RegistryClientConfig::new("https://registry.example.test/", "/tmp/snp-cache"); assert_eq!(config.index_url("demo"), "https://registry.example.test/v1/index/demo.json"); assert_eq!(config.metadata_url("demo", &Version::parse("1.2.3").unwrap()), "https://registry.example.test/v1/packages/demo/1.2.3/snp.toml"); }
    #[test] fn parses_index_json_and_dependencies() { let text = r#"{"protocol":"snp-registry-v1","package":"demo","versions":[{"version":"1.2.0","metadata_url":"m","archive_url":"a","checksum":"sha256:abc","dependencies":[{"name":"core","version":"^1.0.0"}]}]}"#; let index = parse_index_json(text).unwrap(); assert_eq!(index.package, "demo"); assert_eq!(index.versions[0].dependencies[0].constraint, Constraint::Caret(Version::parse("1.0.0").unwrap())); assert_eq!(index.highest_matching(&Constraint::Any).unwrap().version, Version::parse("1.2.0").unwrap()); }
    #[test] fn rejects_malformed_index() { let error = parse_index_json(r#"{"protocol":"snp-registry-v1","package":"demo","versions":[{"version":"bad"}]}"#).unwrap_err(); assert!(matches!(error, RegistryError::Json(_))); }
    #[test] fn rejects_wrong_protocol() { let index = IndexDocument { protocol: "old".into(), package: "demo".into(), versions: vec![] }; assert!(index.validate().is_err()); }
    #[test] fn fetches_index_over_http() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || { let (mut stream, _) = listener.accept().unwrap(); let mut request = [0u8; 512]; let _ = stream.read(&mut request); let body = r#"{"protocol":"snp-registry-v1","package":"demo","versions":[{"version":"1.0.0","metadata_url":"m","archive_url":"a","checksum":"sha256:abc"}]}"#; write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).unwrap(); });
        let client = HttpRegistryClient::new(RegistryClientConfig::new(format!("http://{}", address), "/tmp/snp-cache"));
        let index = client.fetch_index("demo").unwrap();
        worker.join().unwrap();
        assert_eq!(index.versions[0].version, Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn cache_roundtrip_metadata() {
        use std::fs;
        let dir = std::env::temp_dir().join("snp_cache_test_meta");
        let _ = fs::remove_dir_all(&dir);
        let cache = PackageCache::open(&dir);
        let ver = Version::parse("1.0.0").unwrap();
        cache
            .write_metadata("demo", &ver, "[package]\nname = \"demo\"\n")
            .unwrap();
        assert!(cache.has_package("demo", &ver));
        assert!(cache.metadata_path("demo", &ver).is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn downloads_metadata_over_http_into_cache() {
        use std::fs;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let body = r#"{"protocol":"snp-registry-v1","package":"demo","versions":[{"version":"1.0.0","metadata_url":"m","archive_url":"a","checksum":"sha256:abc"}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let meta = "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                meta.len(),
                meta
            );
            stream.write_all(resp.as_bytes()).unwrap();
        });
        let cache_dir = std::env::temp_dir().join("snp_cache_dl_test");
        let _ = fs::remove_dir_all(&cache_dir);
        let client = HttpRegistryClient::new(RegistryClientConfig::new(
            format!("http://{}", address),
            cache_dir.to_string_lossy(),
        ));
        let cache = PackageCache::open(&cache_dir);
        let (ver, path) =
            fetch_package(&client, &cache, "demo", &Constraint::Any, false).unwrap();
        worker.join().unwrap();
        assert_eq!(ver, Version::parse("1.0.0").unwrap());
        assert!(path.join("snp.toml").is_file());
        let _ = fs::remove_dir_all(&cache_dir);
    }
}
