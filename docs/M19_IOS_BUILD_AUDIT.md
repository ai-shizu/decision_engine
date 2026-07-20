# M19-A — iOS Build & Config Audit Checklist (static)

> Companion to `docs/AI_SKILLS.md` §4.22. Do **not** run `tauri ios dev` from
> automation; commander launches the simulator manually.

## Inherited from base `tauri.conf.json` (do not edit base for iOS)

| Field | Value | Status |
|---|---|---|
| `productName` | `PKB` | OK |
| `version` | `0.1.0` | OK (matches `gen/apple` CFBundleShortVersionString) |
| `identifier` | `com.ai-shizu.pkb` | OK (matches Xcode `PRODUCT_BUNDLE_IDENTIFIER`) |
| `app.windows[0].create` | `false` (desktop) | Overridden by `tauri.ios.conf.json` → `true` |
| `bundle.externalBin` | `pkb-engine` (desktop) | Cleared to `[]` on iOS |
| `bundle.targets` | `nsis` (desktop) | Ignored on iOS path |

## `tauri.ios.conf.json` (iOS-only merge)

- bash `beforeDevCommand` / `beforeBuildCommand` (PowerShell base never runs on macOS host)
- `create: true` so the webview mounts
- `externalBin: []` — no Python sidecar on device
- `minimumSystemVersion: 17.0`
- frameworks: `Accelerate` (llama/Metal math) + `LocalAuthentication` (vault Keychain)
- `infoPlist: Info.ios.plist` — Face ID usage string (M3 contract)

## App Sandbox / filesystem permissions

- iOS apps are sandboxed by default. Empty `pkb-desktop_iOS.entitlements` is expected for container-only I/O.
- **No** network client/server entitlements (absolute isolation).
- Vault / logs / knowledge DB must live **inside the app container** (Library / Documents / Application Support under the container). No Full Disk Access / no arbitrary path entitlements.
- Keychain `USER_PRESENCE` requires `NSFaceIDUsageDescription` (provided via `Info.ios.plist`).

## Cargo / target static compatibility (`aarch64-apple-ios` / `aarch64-apple-ios-sim`)

| Crate / feature | iOS note |
|---|---|
| `rusqlite` + `bundled-sqlcipher` | Apple CommonCrypto path; OpenSSL vendored feature **banned** |
| `sqlite-vec` 0.1.9 | Static `SQLITE_CORE` link; **no** `load_extension` / dylib |
| `llama-cpp-2` (`pocket-brain`) | Metal + `common`; needs Metal device capability (already in Info.plist) |
| `objc2-*` / `objc2-ui-kit` | Apple-only / iOS-only for UIKit lifecycle |
| `egress-live` | Optional; keep **off** for offline iOS builds |
| `build.rs` | Skips engine placeholder when `TARGET` contains `ios` |

**Compile proof (M19-A):**
`cargo check --target aarch64-apple-ios-sim --features secure-vault --lib` → exit 0.

## Known follow-up (logic — out of M19-A scope)

`paths.rs::user_data_root()` has no `target_os = "ios"` branch. On iOS it currently falls into the Unix/non-macOS arm (`~/.local/share/PKB` / `XDG_DATA_HOME`), which is **incorrect** for the iOS container. Fix in a later M19 logic ticket — **not** in this config-only phase.
