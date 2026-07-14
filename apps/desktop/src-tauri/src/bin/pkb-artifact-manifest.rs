use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use pkb_desktop_lib::artifact_auth::{
    build_artifact_entry, build_production_manifest, canonical_artifact_manifest_bytes,
    production_public_key, verify_signed_manifest_with_key, ArtifactBase, ArtifactKind,
};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

const PLAN_SCHEMA: &str = "pkb.artifact_plan.v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactPlan {
    schema: String,
    artifacts: Vec<ArtifactPlanEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactPlanEntry {
    base: ArtifactBase,
    id: String,
    kind: ArtifactKind,
    path: String,
}

struct Arguments {
    app_root: PathBuf,
    data_root: PathBuf,
    plan: PathBuf,
    manifest: PathBuf,
    signature: PathBuf,
    target: String,
}

fn fail(message: &str) -> ! {
    eprintln!("pkb-artifact-manifest: {message}");
    std::process::exit(2);
}

fn parse_arguments() -> Arguments {
    let mut values = std::env::args_os().skip(1);
    let mut app_root = None;
    let mut data_root = None;
    let mut plan = None;
    let mut manifest = None;
    let mut signature = None;
    let mut target = None;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .unwrap_or_else(|| fail("every option requires one value"));
        match flag.to_str() {
            Some("--app-root") => app_root = Some(PathBuf::from(value)),
            Some("--data-root") => data_root = Some(PathBuf::from(value)),
            Some("--plan") => plan = Some(PathBuf::from(value)),
            Some("--manifest") => manifest = Some(PathBuf::from(value)),
            Some("--signature") => signature = Some(PathBuf::from(value)),
            Some("--target") => {
                target = Some(
                    value
                        .into_string()
                        .unwrap_or_else(|_| fail("--target must be UTF-8")),
                )
            }
            _ => fail("unknown command-line option"),
        }
    }
    Arguments {
        app_root: app_root.unwrap_or_else(|| fail("--app-root is required")),
        data_root: data_root.unwrap_or_else(|| fail("--data-root is required")),
        plan: plan.unwrap_or_else(|| fail("--plan is required")),
        manifest: manifest.unwrap_or_else(|| fail("--manifest is required")),
        signature: signature.unwrap_or_else(|| fail("--signature is required")),
        target: target.unwrap_or_else(|| fail("--target is required")),
    }
}

fn decode_signing_seed() -> Zeroizing<[u8; 32]> {
    let encoded = Zeroizing::new(
        std::env::var("PKB_ARTIFACT_SIGNING_KEY")
            .unwrap_or_else(|_| fail("PKB_ARTIFACT_SIGNING_KEY is required")),
    );
    std::env::remove_var("PKB_ARTIFACT_SIGNING_KEY");
    let mut decoded = Zeroizing::new(Vec::new());
    if encoded.len() == 64 && encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        for offset in (0..64).step_by(2) {
            decoded.push(
                u8::from_str_radix(&encoded[offset..offset + 2], 16)
                    .unwrap_or_else(|_| fail("signing key encoding is invalid")),
            );
        }
    } else {
        decoded.extend(
            STANDARD
                .decode(encoded.as_bytes())
                .unwrap_or_else(|_| fail("signing key encoding is invalid")),
        );
    }
    if decoded.len() != 32 {
        fail("signing key must be a 32-byte Ed25519 seed");
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&decoded);
    decoded.zeroize();
    seed
}

fn read_plan(path: &Path) -> ArtifactPlan {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|_| fail("plan is unavailable"));
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        fail("plan must be a small regular file");
    }
    let bytes = fs::read(path).unwrap_or_else(|_| fail("plan could not be read"));
    let plan: ArtifactPlan =
        serde_json::from_slice(&bytes).unwrap_or_else(|_| fail("plan JSON is invalid"));
    if plan.schema != PLAN_SCHEMA || plan.artifacts.is_empty() {
        fail("plan schema or artifact list is invalid");
    }
    plan
}

fn atomic_write(path: &Path, bytes: &[u8]) {
    let parent = path
        .parent()
        .unwrap_or_else(|| fail("output parent is unavailable"));
    fs::create_dir_all(parent).unwrap_or_else(|_| fail("output directory could not be created"));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| fail("output filename is invalid"));
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let mut file = File::create_new(&temporary)
        .unwrap_or_else(|_| fail("exclusive temporary output could not be created"));
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .unwrap_or_else(|_| fail("output could not be durably written"));
    drop(file);
    fs::rename(&temporary, path).unwrap_or_else(|_| {
        let _ = fs::remove_file(&temporary);
        fail("output could not be atomically replaced")
    });
}

fn signature_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn main() {
    let args = parse_arguments();
    let plan = read_plan(&args.plan);
    let artifacts = plan
        .artifacts
        .into_iter()
        .map(|entry| {
            let path = entry
                .path
                .replace("{target}", &args.target)
                .replace("{exe}", std::env::consts::EXE_SUFFIX);
            build_artifact_entry(
                entry.id,
                entry.kind,
                entry.base,
                path,
                &args.app_root,
                &args.data_root,
            )
            .unwrap_or_else(|_| fail("planned artifact verification failed"))
        })
        .collect();
    let manifest = build_production_manifest(artifacts)
        .unwrap_or_else(|_| fail("production manifest construction failed"));
    let manifest_bytes = canonical_artifact_manifest_bytes(&manifest)
        .unwrap_or_else(|_| fail("production manifest serialization failed"));

    let seed = decode_signing_seed();
    let signing_key = SigningKey::from_bytes(&seed);
    let production_key =
        production_public_key().unwrap_or_else(|_| fail("production public key is invalid"));
    if signing_key.verifying_key().to_bytes() != production_key {
        fail("signing key does not match the production trust root");
    }
    let signature = signing_key.sign(&manifest_bytes);
    atomic_write(&args.manifest, &manifest_bytes);
    atomic_write(
        &args.signature,
        signature_hex(&signature.to_bytes()).as_bytes(),
    );
    verify_signed_manifest_with_key(
        &args.manifest,
        &args.signature,
        &args.app_root,
        &args.data_root,
        &production_key,
    )
    .unwrap_or_else(|_| fail("written manifest failed independent verification"));
    println!("PKB_ARTIFACT_MANIFEST_SIGNED");
}
