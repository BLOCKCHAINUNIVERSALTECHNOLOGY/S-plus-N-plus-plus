//! S+N++ package manifests, semantic-version constraints and local lockfiles.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version { pub major: u64, pub minor: u64, pub patch: u64 }
impl Version {
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw = text.trim().trim_start_matches('v');
        let parts: Vec<&str> = raw.split('.').collect();
        if parts.len() != 3 { return Err(format!("invalid semantic version `{text}`; expected MAJOR.MINOR.PATCH")); }
        let parse = |part: &str| part.parse::<u64>().map_err(|_| format!("invalid semantic version `{text}`"));
        Ok(Self { major: parse(parts[0])?, minor: parse(parts[1])?, patch: parse(parts[2])? })
    }
}
impl std::fmt::Display for Version { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}.{}.{}", self.major, self.minor, self.patch) } }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint { Any, Exact(Version), Caret(Version), Tilde(Version), Gte(Version) }
impl Constraint {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() || text == "*" { return Ok(Self::Any); }
        if let Some(v) = text.strip_prefix('^') { return Ok(Self::Caret(Version::parse(v)?)); }
        if let Some(v) = text.strip_prefix('~') { return Ok(Self::Tilde(Version::parse(v)?)); }
        if let Some(v) = text.strip_prefix(">=") { return Ok(Self::Gte(Version::parse(v)?)); }
        Ok(Self::Exact(Version::parse(text)?))
    }
    pub fn matches(&self, version: &Version) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => version == expected,
            Self::Gte(min) => version >= min,
            Self::Caret(base) => version >= base && version.major == base.major,
            Self::Tilde(base) => version >= base && version.major == base.major && version.minor == base.minor,
        }
    }
}
impl std::fmt::Display for Constraint { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { match self { Self::Any => write!(f, "*"), Self::Exact(v) => write!(f, "{v}"), Self::Caret(v) => write!(f, "^{v}"), Self::Tilde(v) => write!(f, "~{v}"), Self::Gte(v) => write!(f, ">={v}") } } }

#[derive(Debug, Clone, PartialEq)]
pub struct Dependency { pub name: String, pub source: String, pub constraint: Constraint }
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest { pub name: String, pub version: String, pub entry: String, pub dependencies: BTreeMap<String, Dependency> }
#[derive(Debug, Clone, PartialEq)]
pub struct LockedPackage { pub name: String, pub version: Version, pub source: String, pub checksum: String }
#[derive(Debug, Clone, PartialEq)]
pub struct Lockfile { pub packages: BTreeMap<String, LockedPackage> }

/// A registry is represented by a filesystem root in this milestone. Its layout
/// is intentionally transport-neutral so an HTTP client can use the same paths.
#[derive(Debug, Clone)]
pub struct Registry { pub root: PathBuf }
impl Registry {
    pub fn open(root: impl AsRef<Path>) -> Self { Self { root: root.as_ref().to_path_buf() } }
    pub fn package_manifest(&self, name: &str, version: &Version) -> PathBuf {
        self.root.join(name).join(version.to_string()).join("snp.toml")
    }
    pub fn load_package(&self, name: &str, version: &Version) -> Result<Manifest, String> {
        let path = self.package_manifest(name, version);
        Manifest::load(&path).map_err(|e| format!("registry package `{name}` v{version}: {e}"))
    }
    pub fn available_versions(&self, name: &str) -> Result<Vec<Version>, String> {
        let dir = self.root.join(name);
        let entries = fs::read_dir(&dir).map_err(|e| format!("cannot read registry package `{name}`: {e}"))?;
        let mut versions = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot read registry entry: {e}"))?;
            if entry.path().is_dir() {
                if let Ok(version) = Version::parse(&entry.file_name().to_string_lossy()) { versions.push(version); }
            }
        }
        versions.sort();
        Ok(versions)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedGraph { pub packages: BTreeMap<String, LockedPackage> }

fn parse_inline_dependency(value: &str, name: &str) -> Result<(String, Constraint), String> {
    let value = value.trim();
    if !value.starts_with('{') { return Ok((value.trim_matches('"').to_string(), Constraint::Any)); }
    let inner = value.trim_start_matches('{').trim_end_matches('}');
    let mut source = None; let mut constraint = Constraint::Any;
    for item in inner.split(',') {
        let (key, raw) = item.split_once('=').ok_or_else(|| format!("invalid dependency table for `{name}`"))?;
        let val = raw.trim().trim_matches('"');
        match key.trim() { "path" => source = Some(val.to_string()), "registry" => source = Some(format!("registry:{val}")), "version" => constraint = Constraint::parse(val)?, other => return Err(format!("unknown dependency field `{other}` for `{name}`")) }
    }
    Ok((source.ok_or_else(|| format!("dependency `{name}` is missing path or registry"))?, constraint))
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut section = String::new(); let mut name = None; let mut version = None; let mut entry = None; let mut dependencies = BTreeMap::new();
        for (line_no, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim(); if line.is_empty() { continue; }
            if line.starts_with('[') && line.ends_with(']') { section = line[1..line.len()-1].trim().to_string(); if section != "package" && section != "dependencies" { return Err(format!("unknown manifest section `{section}` at line {}", line_no + 1)); } continue; }
            let (key, value) = line.split_once('=').ok_or_else(|| format!("expected key = value at manifest line {}", line_no + 1))?; let key = key.trim(); let value = value.trim();
            match section.as_str() {
                "package" => match key { "name" => name = Some(value.trim_matches('"').to_string()), "version" => { Version::parse(value.trim_matches('"'))?; version = Some(value.trim_matches('"').to_string()) }, "entry" => entry = Some(value.trim_matches('"').to_string()), _ => return Err(format!("unknown package field `{key}` at line {}", line_no + 1)) },
                "dependencies" => { let (source, constraint) = parse_inline_dependency(value, key)?; dependencies.insert(key.to_string(), Dependency { name: key.to_string(), source, constraint }); },
                _ => return Err(format!("manifest field outside a section at line {}", line_no + 1)),
            }
        }
        Ok(Self { name: name.ok_or("manifest is missing [package] name")?, version: version.ok_or("manifest is missing [package] version")?, entry: entry.unwrap_or_else(|| "src/main.snp".into()), dependencies })
    }
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> { let path = path.as_ref(); Self::parse(&fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?) }
}

fn checksum(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path.join("snp.toml")).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut hash: u64 = 1469598103934665603; for byte in bytes { hash ^= byte as u64; hash = hash.wrapping_mul(1099511628211); } Ok(format!("{hash:016x}"))
}
pub fn resolve_local(root: impl AsRef<Path>, manifest: &Manifest) -> Result<Vec<PathBuf>, String> {
    let root = root.as_ref(); let mut resolved = Vec::new();
    for dependency in manifest.dependencies.values() {
        let source = Path::new(&dependency.source); let path = if source.is_absolute() { source.to_path_buf() } else { root.join(source) };
        if !path.is_dir() { return Err(format!("dependency `{}` path does not exist: {}", dependency.name, path.display())); }
        let dep_manifest = path.join("snp.toml"); if !dep_manifest.is_file() { return Err(format!("dependency `{}` has no snp.toml: {}", dependency.name, path.display())); }
        let dep = Manifest::load(&dep_manifest)?; let actual = Version::parse(&dep.version)?;
        if dep.name != dependency.name { return Err(format!("dependency `{}` resolves to package `{}`", dependency.name, dep.name)); }
        if !dependency.constraint.matches(&actual) { return Err(format!("dependency `{}` requires {}, but local package is {}", dependency.name, dependency.constraint, actual)); }
        resolved.push(path);
    } Ok(resolved)
}

pub fn generate_lock(root: impl AsRef<Path>, manifest: &Manifest) -> Result<Lockfile, String> {
    let root = root.as_ref(); let paths = resolve_local(root, manifest)?; let mut packages = BTreeMap::new();
    for path in paths { let dep = Manifest::load(path.join("snp.toml"))?; packages.insert(dep.name.clone(), LockedPackage { name: dep.name, version: Version::parse(&dep.version)?, source: path.to_string_lossy().to_string(), checksum: checksum(&path)? }); }
        Ok(Lockfile { packages })
}

pub fn generate_lock_with_registry(root: impl AsRef<Path>, manifest: &Manifest, registry: Option<&Registry>) -> Result<Lockfile, String> {
    let root = root.as_ref();
    let mut packages = generate_lock(root, manifest).map(|lock| lock.packages).unwrap_or_default();
    if manifest.dependencies.values().any(|dependency| dependency.source.starts_with("registry:")) {
        let registry = registry.ok_or("registry dependencies require SNP_REGISTRY to point to a registry root")?;
        packages.extend(resolve_registry_graph(registry, manifest)?.packages);
    }
    Ok(Lockfile { packages })
}

pub fn write_lock(path: impl AsRef<Path>, lock: &Lockfile) -> Result<(), String> {
    let mut out = String::from("# S+N++ lockfile v1\n"); for package in lock.packages.values() { out.push_str(&format!("[[package]]\nname = \"{}\"\nversion = \"{}\"\nsource = \"{}\"\nchecksum = \"{}\"\n\n", package.name, package.version, package.source, package.checksum)); } fs::write(path.as_ref(), out).map_err(|e| format!("cannot write {}: {e}", path.as_ref().display()))
}
pub fn load_lock(path: impl AsRef<Path>) -> Result<Lockfile, String> {
    let text = fs::read_to_string(path.as_ref()).map_err(|e| format!("cannot read {}: {e}", path.as_ref().display()))?; let mut packages = BTreeMap::new(); let mut current = BTreeMap::new();
    for raw in text.lines() { let line = raw.trim(); if line == "[[package]]" { if !current.is_empty() { insert_locked(&mut packages, &current)?; current.clear(); } continue; } if let Some((k,v)) = line.split_once('=') { current.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string()); } }
    if !current.is_empty() { insert_locked(&mut packages, &current)?; } Ok(Lockfile { packages })
}
fn insert_locked(packages: &mut BTreeMap<String, LockedPackage>, fields: &BTreeMap<String,String>) -> Result<(), String> { let name = fields.get("name").ok_or("lock package missing name")?.clone(); let version = Version::parse(fields.get("version").ok_or("lock package missing version")?)?; let source = fields.get("source").ok_or("lock package missing source")?.clone(); let checksum = fields.get("checksum").ok_or("lock package missing checksum")?.clone(); packages.insert(name.clone(), LockedPackage { name, version, source, checksum }); Ok(()) }
pub fn validate_lock(root: impl AsRef<Path>, manifest: &Manifest, lock: &Lockfile) -> Result<(), String> {
    validate_lock_with_registry(root, manifest, lock, None)
}

pub fn validate_lock_with_registry(root: impl AsRef<Path>, manifest: &Manifest, lock: &Lockfile, registry: Option<&Registry>) -> Result<(), String> {
    let root = root.as_ref();
    for dependency in manifest.dependencies.values() {
        let item = lock.packages.get(&dependency.name).ok_or_else(|| format!("lockfile is missing dependency `{}`", dependency.name))?;
        if !dependency.constraint.matches(&item.version) { return Err(format!("locked `{}` does not satisfy {}", dependency.name, dependency.constraint)); }
        if item.source.starts_with("registry:") {
            let registry = registry.ok_or("registry lock entries require SNP_REGISTRY")?;
            let name = item.source.strip_prefix("registry:").and_then(|source| source.split('/').next()).unwrap_or("");
            let path = registry.package_manifest(name, &item.version).parent().unwrap().to_path_buf();
            if checksum(&path)? != item.checksum { return Err(format!("checksum mismatch for locked registry package `{}`", dependency.name)); }
        } else {
            let path = Path::new(&item.source);
            if !path.exists() { return Err(format!("locked dependency path does not exist: {}", item.source)); }
            if checksum(path)? != item.checksum { return Err(format!("checksum mismatch for locked dependency `{}`", dependency.name)); }
        }
        let _ = root;
    }
    Ok(())
}

/// Resolve registry dependencies recursively. The first implementation uses one
/// version per package name and reports incompatible constraints deterministically.
pub fn resolve_registry_graph(registry: &Registry, manifest: &Manifest) -> Result<ResolvedGraph, String> {
    let mut packages = BTreeMap::new();
    let mut visiting = Vec::new();
    for dependency in manifest.dependencies.values() {
        if dependency.source.starts_with("registry:") {
            resolve_registry_node(registry, &dependency.name, &dependency.constraint, &mut packages, &mut visiting)?;
        }
    }
    Ok(ResolvedGraph { packages })
}

fn resolve_registry_node(registry: &Registry, name: &str, constraint: &Constraint, packages: &mut BTreeMap<String, LockedPackage>, visiting: &mut Vec<String>) -> Result<(), String> {
    if let Some(locked) = packages.get(name) {
        if !constraint.matches(&locked.version) { return Err(format!("registry version conflict for `{name}`: locked {} does not satisfy {}", locked.version, constraint)); }
        return Ok(());
    }
    if visiting.iter().any(|item| item == name) { return Err(format!("cyclic registry dependency involving `{name}`")); }
    let mut versions = registry.available_versions(name)?;
    versions.retain(|version| constraint.matches(version));
    let version = versions.pop().ok_or_else(|| format!("no registry version of `{name}` satisfies {constraint}"))?;
    let package_path = registry.package_manifest(name, &version).parent().unwrap().to_path_buf();
    let package = registry.load_package(name, &version)?;
    if package.name != name { return Err(format!("registry package `{name}` contains package `{}`", package.name)); }
    visiting.push(name.to_string());
    for dependency in package.dependencies.values() {
        if dependency.source.starts_with("registry:") {
            resolve_registry_node(registry, &dependency.name, &dependency.constraint, packages, visiting)?;
        } else {
            return Err(format!("registry package `{name}` has unsupported local dependency `{}`", dependency.name));
        }
    }
    visiting.pop();
    packages.insert(name.to_string(), LockedPackage { name: name.to_string(), version: version.clone(), source: format!("registry:{name}/{version}"), checksum: checksum(&package_path)? });
    Ok(())
}

pub fn init_project(root: impl AsRef<Path>, name: &str) -> Result<(), String> { let root = root.as_ref(); fs::create_dir_all(root.join("src")).map_err(|e| format!("cannot create project: {e}"))?; let manifest = format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nentry = \"src/main.snp\"\n\n[dependencies]\n"); fs::write(root.join("snp.toml"), manifest).map_err(|e| format!("cannot write snp.toml: {e}"))?; let entry_path = root.join("src/main.snp"); if !entry_path.exists() { fs::write(entry_path, "fn main() {\n    print(\"Hello from S+N++\");\n}\n").map_err(|e| format!("cannot write entry file: {e}"))?; } Ok(()) }

pub fn validate_imports(imports: &[String], manifest: &Manifest) -> Result<(), String> { let std_imports: Vec<String> = imports.iter().filter(|name| name.starts_with("std.")).cloned().collect(); crate::stdlib::resolve(&std_imports)?; for import in imports.iter().filter(|name| !name.starts_with("std.")) { let root = import.split('.').next().unwrap_or(import); if !manifest.dependencies.contains_key(root) { return Err(format!("unknown package namespace `{root}` in import `{import}`; add it to [dependencies]")); } } Ok(()) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_manifest_and_constraints() { let m = Manifest::parse("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n[dependencies]\nutil = { path = \"../util\", version = \"^0.1.0\" }\n").unwrap(); assert_eq!(m.dependencies["util"].constraint, Constraint::Caret(Version { major: 0, minor: 1, patch: 0 })); }
    #[test] fn semver_constraints_match() { let v = Version::parse("0.1.4").unwrap(); assert!(Constraint::parse("^0.1.0").unwrap().matches(&v)); assert!(!Constraint::parse("~0.2.0").unwrap().matches(&v)); }
    #[test] fn rejects_missing_package_name() { assert!(Manifest::parse("[package]\nversion = \"0.1.0\"\n").is_err()); }

    #[test]
    fn resolves_transitive_registry_dependencies() {
        let root = std::env::temp_dir().join(format!("snp-registry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("leaf/1.2.0")).unwrap();
        fs::create_dir_all(root.join("mid/2.0.0")).unwrap();
        fs::write(root.join("leaf/1.2.0/snp.toml"), "[package]\nname = \"leaf\"\nversion = \"1.2.0\"\n").unwrap();
        fs::write(root.join("mid/2.0.0/snp.toml"), "[package]\nname = \"mid\"\nversion = \"2.0.0\"\n[dependencies]\nleaf = { registry = \"default\", version = \"^1.0.0\" }\n").unwrap();
        let manifest = Manifest::parse("[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nmid = { registry = \"default\", version = \"^2.0.0\" }\n").unwrap();
        let graph = resolve_registry_graph(&Registry::open(&root), &manifest).unwrap();
        assert_eq!(graph.packages["mid"].version, Version::parse("2.0.0").unwrap());
        assert_eq!(graph.packages["leaf"].version, Version::parse("1.2.0").unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_registry_version_conflict() {
        let root = std::env::temp_dir().join(format!("snp-registry-conflict-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for version in ["1.0.0", "2.0.0"] {
            fs::create_dir_all(root.join(format!("shared/{version}"))).unwrap();
            fs::write(root.join(format!("shared/{version}/snp.toml")), format!("[package]\nname = \"shared\"\nversion = \"{version}\"\n")).unwrap();
        }
        fs::create_dir_all(root.join("a/1.0.0")).unwrap();
        fs::create_dir_all(root.join("b/1.0.0")).unwrap();
        fs::write(root.join("a/1.0.0/snp.toml"), "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nshared = { registry = \"default\", version = \"^1.0.0\" }\n").unwrap();
        fs::write(root.join("b/1.0.0/snp.toml"), "[package]\nname = \"b\"\nversion = \"1.0.0\"\n[dependencies]\nshared = { registry = \"default\", version = \"^2.0.0\" }\n").unwrap();
        let manifest = Manifest::parse("[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\na = { registry = \"default\", version = \"1.0.0\" }\nb = { registry = \"default\", version = \"1.0.0\" }\n").unwrap();
        let error = resolve_registry_graph(&Registry::open(&root), &manifest).unwrap_err();
        assert!(error.contains("version conflict"));
        let _ = fs::remove_dir_all(root);
    }
}
