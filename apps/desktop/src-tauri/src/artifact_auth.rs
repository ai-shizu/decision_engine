use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const PRODUCTION_PUBLIC_KEY_HEX: &str =
    "3e8f9a2b4c6d5e1f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f";
pub const PRODUCTION_KEY_ID: &str = "pkb-production-ed25519-2026-07";
pub const MANIFEST_SCHEMA: &str = "pkb.artifact_allowlist.v1";

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const TREE_DOMAIN: &[u8] = b"PKB_TREE_V1\0";

#[derive(Debug)]
pub struct ArtifactAuthError(&'static str);

impl std::fmt::Display for ArtifactAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for ArtifactAuthError {}

fn auth_error(message: &'static str) -> ArtifactAuthError {
    ArtifactAuthError(message)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    File,
    Tree,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactBase {
    App,
    Data,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactEntry {
    pub base: ArtifactBase,
    pub id: String,
    pub kind: ArtifactKind,
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    pub artifacts: Vec<ArtifactEntry>,
    pub key_id: String,
    pub schema: String,
}

#[derive(Clone, Debug)]
pub struct VerifiedManifest {
    manifest: ArtifactManifest,
    entries: BTreeMap<String, PathBuf>,
    digest_hex: String,
}

impl VerifiedManifest {
    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }

    pub fn key_id(&self) -> &str {
        &self.manifest.key_id
    }

    pub fn require_exact(&self, id: &str, actual_path: &Path) -> Result<(), ArtifactAuthError> {
        let expected = self
            .entries
            .get(id)
            .ok_or_else(|| auth_error("required artifact is absent from manifest"))?;
        reject_links(actual_path)?;
        let expected = fs::canonicalize(expected)
            .map_err(|_| auth_error("verified artifact path is unavailable"))?;
        let actual = fs::canonicalize(actual_path)
            .map_err(|_| auth_error("required artifact path is unavailable"))?;
        if expected != actual {
            return Err(auth_error("artifact identity does not match manifest"));
        }
        Ok(())
    }
}

pub struct ProductionAttestation {
    pub manifest_path: PathBuf,
    pub manifest_digest_hex: String,
    pub app_root: PathBuf,
}

pub fn build_artifact_entry(
    id: String,
    kind: ArtifactKind,
    base: ArtifactBase,
    path: String,
    app_root: &Path,
    data_root: &Path,
) -> Result<ArtifactEntry, ArtifactAuthError> {
    let unsigned = ArtifactEntry {
        base,
        id,
        kind,
        path,
        sha256: "0".repeat(64),
        size: 0,
    };
    let resolved = resolve_entry(&unsigned, app_root, data_root)?;
    let (sha256, size) = match kind {
        ArtifactKind::File => {
            let metadata = fs::symlink_metadata(&resolved)
                .map_err(|_| auth_error("planned artifact is unavailable"))?;
            reject_reparse_metadata(&metadata)?;
            if !metadata.is_file() {
                return Err(auth_error("planned artifact is not a regular file"));
            }
            (sha256_file_hex(&resolved)?, metadata.len())
        }
        ArtifactKind::Tree => sha256_tree_hex(&resolved)?,
    };
    Ok(ArtifactEntry {
        sha256,
        size,
        ..unsigned
    })
}

pub fn build_production_manifest(
    mut artifacts: Vec<ArtifactEntry>,
) -> Result<ArtifactManifest, ArtifactAuthError> {
    artifacts.sort_by(|left, right| left.id.cmp(&right.id));
    let manifest = ArtifactManifest {
        artifacts,
        key_id: PRODUCTION_KEY_ID.to_string(),
        schema: MANIFEST_SCHEMA.to_string(),
    };
    validate_manifest(&manifest)?;
    validate_production_inventory(&manifest)?;
    Ok(manifest)
}

pub fn canonical_artifact_manifest_bytes(
    manifest: &ArtifactManifest,
) -> Result<Vec<u8>, ArtifactAuthError> {
    let value =
        serde_json::to_value(manifest).map_err(|_| auth_error("manifest serialization failed"))?;
    canonical_manifest_bytes(&value)
}

pub fn canonical_manifest_bytes(value: &Value) -> Result<Vec<u8>, ArtifactAuthError> {
    reject_noncanonical_json_types(value)?;
    let sorted = sort_json_objects(value);
    serde_json::to_vec(&sorted).map_err(|_| auth_error("manifest canonicalization failed"))
}

fn sort_json_objects(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(sort_json_objects).collect()),
        Value::Object(object) => {
            let ordered = object
                .iter()
                .map(|(key, value)| (key.clone(), sort_json_objects(value)))
                .collect::<BTreeMap<_, _>>();
            let mut sorted = serde_json::Map::new();
            for (key, value) in ordered {
                sorted.insert(key, value);
            }
            Value::Object(sorted)
        }
        _ => value.clone(),
    }
}

fn reject_noncanonical_json_types(value: &Value) -> Result<(), ArtifactAuthError> {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
        Value::Number(number) if number.is_u64() => Ok(()),
        Value::Number(_) => Err(auth_error("manifest numbers must be unsigned integers")),
        Value::Array(items) => {
            for item in items {
                reject_noncanonical_json_types(item)?;
            }
            Ok(())
        }
        Value::Object(object) => {
            for item in object.values() {
                reject_noncanonical_json_types(item)?;
            }
            Ok(())
        }
    }
}

fn read_small_regular_file(path: &Path, maximum: u64) -> Result<Vec<u8>, ArtifactAuthError> {
    reject_links(path)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| auth_error("required integrity file is unavailable"))?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(auth_error("integrity file has an invalid type or size"));
    }
    fs::read(path).map_err(|_| auth_error("integrity file could not be read"))
}

fn decode_hex_exact<const N: usize>(raw: &str) -> Result<[u8; N], ArtifactAuthError> {
    if raw.len() != N * 2
        || !raw
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(auth_error("hex value has an invalid shape"));
    }
    let mut output = [0u8; N];
    for (index, slot) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *slot = u8::from_str_radix(&raw[offset..offset + 2], 16)
            .map_err(|_| auth_error("hex value is invalid"))?;
    }
    Ok(output)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub fn sha256_file_hex(path: &Path) -> Result<String, ArtifactAuthError> {
    reject_links(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| auth_error("artifact is unavailable"))?;
    if !metadata.is_file() {
        return Err(auth_error("artifact is not a regular file"));
    }
    let file = File::open(path).map_err(|_| auth_error("artifact could not be opened"))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| auth_error("artifact could not be hashed"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex(&hasher.finalize()))
}

fn tree_files(root: &Path) -> Result<Vec<(String, PathBuf, u64)>, ArtifactAuthError> {
    reject_links(root)?;
    let metadata =
        fs::symlink_metadata(root).map_err(|_| auth_error("artifact tree is unavailable"))?;
    if !metadata.is_dir() {
        return Err(auth_error("artifact tree is not a directory"));
    }

    fn walk(
        root: &Path,
        current: &Path,
        output: &mut Vec<(String, PathBuf, u64)>,
    ) -> Result<(), ArtifactAuthError> {
        let mut entries = fs::read_dir(current)
            .map_err(|_| auth_error("artifact tree could not be enumerated"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| auth_error("artifact tree entry could not be read"))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| auth_error("artifact tree metadata failed"))?;
            reject_reparse_metadata(&metadata)?;
            if metadata.is_dir() {
                walk(root, &path, output)?;
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| auth_error("artifact tree escaped its root"))?;
                let relative = relative
                    .components()
                    .map(|component| {
                        component
                            .as_os_str()
                            .to_str()
                            .ok_or_else(|| auth_error("artifact tree path is not UTF-8"))
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("/");
                validate_relative_path(&relative)?;
                output.push((relative, path, metadata.len()));
            } else {
                return Err(auth_error("artifact tree contains a special file"));
            }
        }
        Ok(())
    }

    let mut output = Vec::new();
    walk(root, root, &mut output)?;
    output.sort_by(|left, right| left.0.cmp(&right.0));
    if output.is_empty() {
        return Err(auth_error("artifact tree is empty"));
    }
    Ok(output)
}

pub fn sha256_tree_hex(path: &Path) -> Result<(String, u64), ArtifactAuthError> {
    let files = tree_files(path)?;
    let mut hasher = Sha256::new();
    hasher.update(TREE_DOMAIN);
    let mut total_size = 0u64;
    for (relative, file, size) in files {
        let relative = relative.as_bytes();
        hasher.update((relative.len() as u64).to_be_bytes());
        hasher.update(relative);
        hasher.update(size.to_be_bytes());
        let digest = decode_hex_exact::<32>(&sha256_file_hex(&file)?)?;
        hasher.update(digest);
        total_size = total_size
            .checked_add(size)
            .ok_or_else(|| auth_error("artifact tree size overflow"))?;
    }
    Ok((hex(&hasher.finalize()), total_size))
}

fn validate_relative_path(raw: &str) -> Result<(), ArtifactAuthError> {
    if raw.is_empty()
        || raw.contains('\\')
        || raw.contains(':')
        || raw.starts_with('/')
        || raw.ends_with('/')
        || raw.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
        || raw
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(auth_error(
            "artifact path is not a normalized relative path",
        ));
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(auth_error("artifact path is not relative"));
    }
    Ok(())
}

fn resolve_entry(
    entry: &ArtifactEntry,
    app_root: &Path,
    data_root: &Path,
) -> Result<PathBuf, ArtifactAuthError> {
    validate_relative_path(&entry.path)?;
    let base = match entry.base {
        ArtifactBase::App => app_root,
        ArtifactBase::Data => data_root,
    };
    let mut resolved = base.to_path_buf();
    for component in entry.path.split('/') {
        resolved.push(component);
    }
    Ok(resolved)
}

fn validate_manifest(manifest: &ArtifactManifest) -> Result<(), ArtifactAuthError> {
    if manifest.schema != MANIFEST_SCHEMA || manifest.key_id.trim().is_empty() {
        return Err(auth_error("manifest schema or key id is invalid"));
    }
    if manifest.artifacts.is_empty() || manifest.artifacts.len() > 4096 {
        return Err(auth_error("manifest artifact count is invalid"));
    }
    let mut ids = BTreeSet::new();
    let mut previous: Option<&str> = None;
    for entry in &manifest.artifacts {
        if entry.id.is_empty()
            || entry.id.len() > 128
            || !entry
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
            || !ids.insert(entry.id.as_str())
        {
            return Err(auth_error("manifest artifact id is invalid or duplicated"));
        }
        if previous.is_some_and(|value| value >= entry.id.as_str()) {
            return Err(auth_error(
                "manifest artifacts are not in canonical id order",
            ));
        }
        previous = Some(&entry.id);
        validate_relative_path(&entry.path)?;
        decode_hex_exact::<32>(&entry.sha256)?;
    }
    Ok(())
}

fn validate_production_inventory(manifest: &ArtifactManifest) -> Result<(), ArtifactAuthError> {
    for required in [
        "engine_sidecar",
        "search_engine",
        "llama_runtime",
        "config:model_params",
        "release_sbom",
    ] {
        if !manifest.artifacts.iter().any(|entry| entry.id == required) {
            return Err(auth_error("production manifest omits a required artifact"));
        }
    }
    if !manifest
        .artifacts
        .iter()
        .any(|entry| entry.id == "gguf" || entry.id.starts_with("gguf:"))
    {
        return Err(auth_error("production manifest omits a required model"));
    }
    Ok(())
}

fn verify_entry(
    entry: &ArtifactEntry,
    app_root: &Path,
    data_root: &Path,
) -> Result<PathBuf, ArtifactAuthError> {
    let path = resolve_entry(entry, app_root, data_root)?;
    let (digest, size) = match entry.kind {
        ArtifactKind::File => {
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| auth_error("manifest artifact is missing"))?;
            reject_reparse_metadata(&metadata)?;
            if !metadata.is_file() {
                return Err(auth_error("manifest artifact type mismatch"));
            }
            (sha256_file_hex(&path)?, metadata.len())
        }
        ArtifactKind::Tree => sha256_tree_hex(&path)?,
    };
    if size != entry.size || digest != entry.sha256 {
        return Err(auth_error("manifest artifact digest or size mismatch"));
    }
    Ok(path)
}

pub fn verify_signed_manifest_with_key(
    manifest_path: &Path,
    signature_path: &Path,
    app_root: &Path,
    data_root: &Path,
    public_key: &[u8; 32],
) -> Result<VerifiedManifest, ArtifactAuthError> {
    let manifest_bytes = read_small_regular_file(manifest_path, MAX_MANIFEST_BYTES)?;
    let value: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| auth_error("manifest JSON is invalid"))?;
    let canonical = canonical_manifest_bytes(&value)?;
    if canonical != manifest_bytes {
        return Err(auth_error("manifest is not canonical JSON"));
    }

    let signature_text = String::from_utf8(read_small_regular_file(signature_path, 128)?)
        .map_err(|_| auth_error("manifest signature is not UTF-8"))?;
    let signature_bytes = decode_hex_exact::<64>(signature_text.trim())?;
    if signature_text.as_bytes().len() != 128 {
        return Err(auth_error("manifest signature has trailing data"));
    }
    let verifying_key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| auth_error("artifact trust root is invalid"))?;
    let signature = Signature::from_bytes(&signature_bytes);
    verifying_key
        .verify_strict(&canonical, &signature)
        .map_err(|_| auth_error("manifest signature verification failed"))?;

    let manifest: ArtifactManifest =
        serde_json::from_value(value).map_err(|_| auth_error("manifest structure is invalid"))?;
    validate_manifest(&manifest)?;

    let mut entries = BTreeMap::new();
    for entry in &manifest.artifacts {
        let path = verify_entry(entry, app_root, data_root)?;
        entries.insert(entry.id.clone(), path);
    }
    let digest_hex = hex(&Sha256::digest(&canonical));
    Ok(VerifiedManifest {
        manifest,
        entries,
        digest_hex,
    })
}

pub fn production_public_key() -> Result<[u8; 32], ArtifactAuthError> {
    decode_hex_exact(PRODUCTION_PUBLIC_KEY_HEX)
}

pub fn verify_production_artifacts(
    data_root: &Path,
    sidecar_path: &Path,
) -> Result<ProductionAttestation, ArtifactAuthError> {
    let app_root = sidecar_path
        .parent()
        .ok_or_else(|| auth_error("production application root is unavailable"))?;
    let manifest_path = data_root.join("config").join("artifacts_allowlist.json");
    let signature_path = data_root.join("config").join("artifacts_allowlist.sig");
    let public_key = production_public_key()?;
    let verified = verify_signed_manifest_with_key(
        &manifest_path,
        &signature_path,
        app_root,
        data_root,
        &public_key,
    )?;
    if verified.key_id() != PRODUCTION_KEY_ID {
        return Err(auth_error(
            "manifest key id is not the production trust root",
        ));
    }
    validate_production_inventory(&verified.manifest)?;
    verified.require_exact("engine_sidecar", sidecar_path)?;
    Ok(ProductionAttestation {
        manifest_path,
        manifest_digest_hex: verified.digest_hex().to_string(),
        app_root: app_root.to_path_buf(),
    })
}

fn reject_links(path: &Path) -> Result<(), ArtifactAuthError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| auth_error("artifact path component is unavailable"))?;
        reject_reparse_metadata(&metadata)?;
    }
    Ok(())
}

fn reject_reparse_metadata(metadata: &fs::Metadata) -> Result<(), ArtifactAuthError> {
    if metadata.file_type().is_symlink() {
        return Err(auth_error("symbolic links are forbidden in artifact paths"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(auth_error("reparse points are forbidden in artifact paths"));
        }
    }
    Ok(())
}
