use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::{Signer, SigningKey};
use pkb_desktop_lib::artifact_auth::{
    canonical_manifest_bytes, sha256_file_hex, verify_signed_manifest_with_key,
    PRODUCTION_PUBLIC_KEY_HEX,
};
use serde_json::{json, Value};

const TEST_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_root() -> PathBuf {
    let sequence = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("pkb-fsa04-{}-{sequence}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).expect("remove stale temp root");
    }
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn signed_manifest(root: &Path, manifest: Value, signing_key: &SigningKey) -> (PathBuf, PathBuf) {
    let manifest_path = root.join("artifacts_allowlist.json");
    let signature_path = root.join("artifacts_allowlist.sig");
    let canonical = canonical_manifest_bytes(&manifest).expect("canonical manifest");
    fs::write(&manifest_path, &canonical).expect("write manifest");
    fs::write(
        &signature_path,
        hex(&signing_key.sign(&canonical).to_bytes()),
    )
    .expect("write signature");
    (manifest_path, signature_path)
}

fn one_file_manifest(asset_path: &Path, digest: String, size: u64) -> Value {
    json!({
        "artifacts": [{
            "base": "app",
            "id": "engine_sidecar",
            "kind": "file",
            "path": asset_path.file_name().unwrap().to_str().unwrap(),
            "sha256": digest,
            "size": size
        }],
        "key_id": "pkb-test-ed25519-rfc8032",
        "schema": "pkb.artifact_allowlist.v1"
    })
}

#[test]
fn production_root_is_fixed_and_not_the_public_test_vector() {
    assert_eq!(
        PRODUCTION_PUBLIC_KEY_HEX,
        "3e8f9a2b4c6d5e1f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f"
    );
    assert_ne!(
        PRODUCTION_PUBLIC_KEY_HEX,
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
}

#[test]
fn valid_test_signature_and_exact_asset_are_accepted() {
    let root = temp_root();
    let app = root.join("app");
    let data = root.join("data");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&data).unwrap();
    let asset = app.join("pkb-engine-test.exe");
    fs::write(&asset, b"trusted-sidecar").unwrap();
    let manifest = one_file_manifest(
        &asset,
        sha256_file_hex(&asset).unwrap(),
        fs::metadata(&asset).unwrap().len(),
    );
    let signing_key = SigningKey::from_bytes(&TEST_SEED);
    let (manifest_path, signature_path) = signed_manifest(&root, manifest, &signing_key);

    let verified = verify_signed_manifest_with_key(
        &manifest_path,
        &signature_path,
        &app,
        &data,
        &signing_key.verifying_key().to_bytes(),
    )
    .expect("valid signed manifest");
    verified
        .require_exact("engine_sidecar", &asset)
        .expect("exact sidecar binding");
}

#[test]
fn unsigned_wrong_key_and_one_byte_tamper_are_rejected() {
    let root = temp_root();
    let app = root.join("app");
    let data = root.join("data");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&data).unwrap();
    let asset = app.join("model.gguf");
    fs::write(&asset, b"valid-model").unwrap();
    let manifest = one_file_manifest(
        &asset,
        sha256_file_hex(&asset).unwrap(),
        fs::metadata(&asset).unwrap().len(),
    );
    let signing_key = SigningKey::from_bytes(&TEST_SEED);
    let (manifest_path, signature_path) = signed_manifest(&root, manifest, &signing_key);

    fs::remove_file(&signature_path).unwrap();
    assert!(verify_signed_manifest_with_key(
        &manifest_path,
        &signature_path,
        &app,
        &data,
        &signing_key.verifying_key().to_bytes(),
    )
    .is_err());

    let (_, signature_path) = signed_manifest(
        &root,
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap(),
        &signing_key,
    );
    assert!(verify_signed_manifest_with_key(
        &manifest_path,
        &signature_path,
        &app,
        &data,
        &[0x11; 32],
    )
    .is_err());

    fs::write(&asset, b"valid-modeL").unwrap();
    assert!(verify_signed_manifest_with_key(
        &manifest_path,
        &signature_path,
        &app,
        &data,
        &signing_key.verifying_key().to_bytes(),
    )
    .is_err());
}

#[test]
fn traversal_and_noncanonical_manifest_are_rejected() {
    let root = temp_root();
    let app = root.join("app");
    let data = root.join("data");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&data).unwrap();
    let signing_key = SigningKey::from_bytes(&TEST_SEED);
    let manifest = json!({
        "schema": "pkb.artifact_allowlist.v1",
        "key_id": "pkb-test-ed25519-rfc8032",
        "artifacts": [{
            "id": "escape",
            "kind": "file",
            "base": "data",
            "path": "../escape.bin",
            "sha256": "00".repeat(32),
            "size": 1
        }]
    });
    let (manifest_path, signature_path) = signed_manifest(&root, manifest, &signing_key);
    assert!(verify_signed_manifest_with_key(
        &manifest_path,
        &signature_path,
        &app,
        &data,
        &signing_key.verifying_key().to_bytes(),
    )
    .is_err());

    let parsed: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let pretty = serde_json::to_vec_pretty(&parsed).unwrap();
    fs::write(&manifest_path, &pretty).unwrap();
    fs::write(&signature_path, hex(&signing_key.sign(&pretty).to_bytes())).unwrap();
    assert!(verify_signed_manifest_with_key(
        &manifest_path,
        &signature_path,
        &app,
        &data,
        &signing_key.verifying_key().to_bytes(),
    )
    .is_err());
}

#[test]
fn production_signer_rejects_public_test_seed_without_output_or_secret_echo() {
    let root = temp_root();
    let app = root.join("app");
    let data = root.join("data");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(data.join("runtime")).unwrap();
    fs::create_dir_all(data.join("sbom")).unwrap();
    fs::write(app.join("sidecar"), b"sidecar").unwrap();
    fs::write(data.join("search"), b"search").unwrap();
    fs::write(data.join("model.gguf"), b"model").unwrap();
    fs::write(data.join("config.json"), b"config").unwrap();
    fs::write(data.join("runtime/llama"), b"runtime").unwrap();
    fs::write(data.join("sbom/release.cdx.json"), b"sbom").unwrap();
    let plan = root.join("plan.json");
    fs::write(
        &plan,
        br#"{"artifacts":[{"base":"data","id":"config:model_params","kind":"file","path":"config.json"},{"base":"app","id":"engine_sidecar","kind":"file","path":"sidecar"},{"base":"data","id":"gguf:consult","kind":"file","path":"model.gguf"},{"base":"data","id":"llama_runtime","kind":"tree","path":"runtime"},{"base":"data","id":"release_sbom","kind":"tree","path":"sbom"},{"base":"data","id":"search_engine","kind":"file","path":"search"}],"schema":"pkb.artifact_plan.v1"}"#,
    )
    .unwrap();
    let manifest = root.join("manifest.json");
    let signature = root.join("manifest.sig");
    let public_test_seed = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    let output = Command::new(env!("CARGO_BIN_EXE_pkb-artifact-manifest"))
        .args([
            "--app-root",
            app.to_str().unwrap(),
            "--data-root",
            data.to_str().unwrap(),
            "--plan",
            plan.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
            "--signature",
            signature.to_str().unwrap(),
            "--target",
            "test-target",
        ])
        .env("PKB_ARTIFACT_SIGNING_KEY", public_test_seed)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!manifest.exists());
    assert!(!signature.exists());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(public_test_seed));
}
