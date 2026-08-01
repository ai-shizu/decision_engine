# M3 Action Plan: SQLCipher Canonical Vault

Status: **Architecture decision / implementation blueprint**

Scope: iOS standalone target only

Decision date: 2026-07-18

## 0. Executive decision

M3 adopts an **iOS Keychain-backed, device-bound 256-bit SQLCipher key** and a
**Rust-owned, single-connection vault worker**.

- Generate exactly 32 random bytes with `SecRandomCopyBytes` through the
  `security-framework` API when a brand-new vault is provisioned.
- Store the key as a generic-password Keychain item protected by
  `kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly`, `userPresence`,
  `kSecAttrSynchronizable=false`, and the Data Protection Keychain.
- Let iOS satisfy `userPresence` with Face ID, Touch ID, or the device passcode.
  Do not implement an application-owned biometric prompt or collect a vault
  password in the WebView.
- Keep plaintext key bytes only inside a `Zeroizing<[u8; 32]>` owned by the
  dedicated vault worker while the vault is unlocked. Never return the key from
  that worker or place it in Tauri State, JavaScript, UserDefaults, logs, panic
  text, telemetry, or recovery metadata.
- Close the SQLCipher connection and zeroize the in-memory key when the app is
  explicitly locked, enters the background, or protected data becomes
  unavailable.
- Use `rusqlite` with `bundled-sqlcipher` and SQLCipher's Apple/CommonCrypto
  path. Do not introduce OpenSSL or `bundled-sqlcipher-vendored-openssl`.
- A single Rust worker owns the only SQLCipher connection. “Connection pool” in
  M3 means a serialized executor with capacity one, not a multi-connection pool.
  Python, UIKit, Tauri commands, and the WebView never open the vault.

Secure Enclave is **not** the SQLCipher-key store. It protects P-256 private-key
operations, not arbitrary imported 32-byte symmetric secrets. It remains the
preferred home of the journal-signing key described by the roadmap.

## 1. Decision record

### 1.1 Security and UX trade-off

| Option | UX | Security / operational property | Decision |
|---|---|---|---|
| Ask for an app password at every launch | High friction | Human-derived entropy requires a KDF and introduces forgotten-password support | Rejected as the device-vault default |
| Keychain without user presence | Seamless | Any code running as the app while the device is unlocked can retrieve the key | Rejected |
| Keychain + `biometryCurrentSet` | Low friction | Biometric enrollment changes invalidate access and can strand the vault | Rejected for the database key |
| Keychain + `WhenPasscodeSetThisDeviceOnly` + `userPresence` | One trusted OS ceremony per foreground unlock | Device-bound, requires passcode-enabled device, allows biometric or device-passcode fallback | **Selected** |
| Secure Enclave-wrapped/imported SQLCipher key | Attractive in theory | Secure Enclave does not store/import an arbitrary symmetric database key directly | Rejected for DB key; retained for journal signer |

The selected policy gives a system-owned authentication ceremony without
making the user remember another secret. An app password may later protect the
portable M6 recovery archive, but it must be a separate key hierarchy and must
not replace the random device-vault key.

### 1.2 Threat model

M3 is designed to resist:

- offline extraction of the app container or an iCloud/device backup;
- disclosure of a database copied without its ThisDeviceOnly Keychain item;
- accidental key leakage through the WebView, IPC payloads, logs, crash text,
  telemetry, or configuration;
- plaintext SQLite files, temporary spill files, WAL content, and migration
  intermediates;
- arbitrary SQL/PRAGMA injection from the frontend;
- two runtimes concurrently owning the canonical database;
- use of a wrong, missing, or replaced key as an excuse to silently recreate or
  overwrite an existing vault.

M3 does not claim to resist a fully compromised, unlocked OS process, hardware
laboratory attacks outside Apple's platform guarantees, or coherent rollback of
both the DB and all Keychain anchors. Recovery from device loss is an M6 concern.

### 1.3 Fail-closed vault state matrix

| Keychain item | DB file | Required result |
|---|---|---|
| absent | absent | `Unprovisioned`; explicit first-vault creation may generate a key |
| present | present | `Locked`; successful OS user-presence authentication may unlock |
| absent | present | `RecoveryRequired`; never generate a replacement key or overwrite DB |
| present | absent | `OrphanedKey`; never silently create a new vault under the old identity |
| auth cancelled/failed | any | remain `Locked`; fixed typed error, no fallback credential path |
| wrong key / integrity failure | present | `Quarantined`; read/write operations disabled |

Removal of the device passcode can invalidate the selected Keychain item. Until
M6 recovery exists, the UI and release notes must state that device loss,
passcode removal, or Keychain loss can make the vault unrecoverable. This risk
must not be hidden by automatic re-provisioning.

### 1.4 Rust and Tauri boundary

The Apple key provider is a narrow, target-gated Rust module built on
`security-framework`:

- proposed pin: `security-framework = "=3.7.0"`,
  `default-features = false`, optional, Apple targets only;
- create access control with the fallible
  `SecAccessControl::create_with_protection(...)` path, then apply it to
  `PasswordOptions`; do not use a convenience path that internally panics;
- set synchronizable explicitly to `false` and select the Data Protection
  Keychain;
- generate bytes with the fallible `SecRandom` wrapper;
- use the app's private access group only; do not add a shared Keychain group;
- expose only `provision`, `unlock`, `lock`, `status`, and narrowly typed journal
  signing/anchor operations. There is no `get_raw_key` Tauri command.

If the high-level crate cannot express a required query attribute, Phase 0 may
add the smallest audited wrapper over `security-framework-sys`. No third-party
Tauri credential plugin may weaken or abstract away the exact accessibility,
synchronization, access-control, and Data Protection attributes.

The Tauri-managed value is only a cloneable `VaultHandle` containing a command
sender and public lock status. The vault thread exclusively owns:

- the plaintext key buffer;
- `rusqlite::Connection`;
- migration state and transactions;
- Keychain anchor transitions associated with committed journal events.

This follows the same ownership lesson as the non-`Send` LLM context: capability
handles cross thread boundaries; sensitive state does not.

### 1.5 Canonical connection owner — roadmap delta

`ROADMAP_IOS_TAURI_V2.md` currently names a serialized Python worker as the
baseline SQLCipher owner and allows Rust ownership only after re-review. This
document is that re-review and selects the Rust-owned alternative because the
current iOS Tauri command, M5 extraction, lifecycle, and Keychain boundaries are
already Rust-owned and M3 explicitly requires a Rust DB layer.

Before production implementation begins, the roadmap must be amended to match
this ADR. The following invariants are non-negotiable:

1. Rust is the sole iOS SQLCipher connection owner.
2. Python `_sqlite3`, system SQLite, JavaScript SQL plugins, UIKit, and WebView do
   not open `vault.sqlite`.
3. No transition release contains both Python and Rust vault owners.
4. Desktop keeps its existing file-backed store until a separate migration is
   approved.

## 2. SQLCipher build and link strategy

### 2.1 Proposed Cargo isolation

Use a new opt-in `secure-vault` feature so default desktop builds remain
unchanged. The Phase 0 candidate dependency is:

```toml
rusqlite = { version = "=0.40.1", default-features = false, features = ["bundled-sqlcipher"], optional = true }
```

On Apple targets, add the pinned `security-framework` dependency described in
§1.4. `secure-vault` enables only these optional dependencies. Do not use SQLx,
`tauri-plugin-sql`, r2d2, system SQLite, or an OpenSSL-vendored SQLCipher feature.

`bundled-sqlcipher` is selected because it builds one known SQLCipher source
inside the Rust dependency graph and uses Apple's crypto path on iOS. Phase 0
must verify the resolved versions, SQLCipher license/SBOM, symbols, compile
options, and final Xcode link rather than trusting `cargo check`.

### 2.2 Key application and open order

Required open order:

1. resolve the fixed app-container vault path;
2. establish file protection and backup exclusion on the vault directory;
3. open the connection with a fixed flag set and no frontend-controlled URI;
4. apply the raw 32-byte SQLCipher key **before the first database operation**;
5. immediately verify access to `sqlite_master` and SQLCipher identity;
6. set and verify fixed PRAGMAs;
7. run cipher integrity checks before migration or repository access;
8. migrate inside a transaction, then publish `Unlocked`.

Prefer SQLCipher's binary key API so key bytes never become SQL text. Phase 0
must inspect the exact `libsqlite3-sys` surface for the pinned versions. If
`sqlite3_key` is not bound, add one minimal reviewed FFI declaration; do not
invent a rusqlite method. A raw-key PRAGMA is only a fallback after security
review and must use a zeroized buffer with no formatting/logging exposure.

### 2.3 Fixed database controls

The connection initializer owns a closed allowlist of settings; the frontend
cannot submit SQL or PRAGMAs. At minimum, Phase 0/1 must verify:

- `PRAGMA cipher_version` returns a non-empty SQLCipher version;
- `PRAGMA compile_options` matches the approved artifact receipt;
- `PRAGMA foreign_keys = ON`;
- `PRAGMA trusted_schema = OFF`;
- `PRAGMA secure_delete = ON`;
- `PRAGMA journal_mode = WAL`;
- `PRAGMA synchronous = FULL`;
- `PRAGMA temp_store = MEMORY`;
- cipher integrity check succeeds;
- opening with a wrong key fails without mutating the file.

The release artifact must demonstrate one SQLite implementation and no
ambiguous system-SQLite/SQLCipher symbol resolution. Database, WAL, SHM, and any
temporary path receive iOS Data Protection and backup exclusion. Sort/query
bounds must prevent unexpected disk spill; plaintext scans must include all
sidecars and staging directories.

## 3. Target module layout

```text
apps/desktop/src-tauri/src/
├── domain/
│   └── kakeibo.rs             # persistence-domain types; no LLM dependency
├── vault/
│   ├── mod.rs                 # public VaultHandle and state machine
│   ├── error.rs               # closed, serializable public error codes
│   ├── worker.rs              # sole DB/key owner; serialized request loop
│   ├── key_provider.rs        # trait and Apple Keychain implementation
│   ├── path.rs                # fixed app-container paths/protection/exclusion
│   ├── connection.rs          # SQLCipher open/key/verify/PRAGMA sequence
│   ├── migration.rs           # immutable migrations and receipts
│   ├── journal.rs             # reservation, signed event, anchor protocol
│   ├── repository.rs          # closed repository operations
│   └── commands_vault.rs      # Tauri commands; no SQL/key parameters
└── llm/
    └── schema.rs              # extraction DTO converts to domain type

apps/desktop/src/
├── lib/vault.ts               # typed status/unlock/save IPC only
├── lib/extractionSink.ts      # adds a vault sink; clipboard sink remains
├── reducers/vaultReducer.ts   # pure lock/save state transitions
└── components/VaultPanel.tsx  # Dumb View and OS-auth ceremony handoff
```

Exact paths may follow existing frontend conventions, but responsibility
boundaries may not be collapsed.

## 4. Phase 0 — native viability gate (HARD STOP)

No schema, repository, or production UI logic is written until all Phase 0
gates pass.

### 4.1 Dependency and artifact gate

1. Create an isolated M3 branch from synchronized `main`.
2. Record `rustc`, Cargo, Xcode, iOS SDK, `rusqlite`, `libsqlite3-sys`, SQLCipher,
   and `security-framework` exact versions.
3. Resolve only the `secure-vault` feature and generate an SBOM/license receipt.
4. Confirm no OpenSSL/native-tls dependency or system SQLite enters the iOS link.
5. Inspect build output and final link map for SQLCipher and Apple
   Security/CommonCrypto symbols. `cargo check` is not evidence of final link.

### 4.2 Keychain probe

Build a disposable, feature-gated native probe that:

1. generates a 32-byte key using `SecRandom`;
2. creates an access-control object for
   `AccessibleWhenPasscodeSetThisDeviceOnly` + `USER_PRESENCE` without
   `unwrap`/`expect`;
3. stores a non-synchronizable generic-password item in the Data Protection
   Keychain;
4. retrieves it through a real system authentication ceremony;
5. verifies cancellation and authentication failure produce typed errors;
6. deletes the probe item and zeroizes all buffers.

Run this on the iOS simulator for plumbing and on a physical passcode-enabled
device for the security acceptance. Verify `NSFaceIDUsageDescription` and the
system prompt copy before enabling Face ID. Simulator success alone is not the
final security gate.

#### 4.2.1 User-approved Keychain probe contract (2026-07-18)

Formal name for this workstream: **M3 Phase 0 / §4.2 — Keychain probe**.
Do not invent alternate phase labels (for example “Phase 0-C”).

This subsection records the user-approved contract and the independently
verified ownership/type-feasibility result. It does **not** authorize
implementation in the same change, and it does **not** prove main-repository
integration, final-app link, or runtime behavior for LocalAuthentication.

##### Exact prompt copy

| Key | Exact approved string (byte-for-byte; do not paraphrase) |
|---|---|
| `NSFaceIDUsageDescription` | `Vaultの自動解錠および、機密性の高いプロンプトデータの保護にFace IDを使用します。` |
| `LAContext.localizedReason` | `暗号化されたデータのロックを解除します。` |
| `NSCalendarsFullAccessUsageDescription` (added 2026-07-23, M20 Part 2 EventKit bridge) | `Coraxisのコンテキストに予定を取り込むため、カレンダーへのフルアクセスを使用します。` |

Changing any of these strings requires a new user ruling. Do not append the
app name or other suffixes to `localizedReason`.

##### Explicit-action harness contract (`KeychainProbePanel`)

A Phase 0-only debug harness named `KeychainProbePanel` is permitted in a later
limited implementation under these rules:

- Start the probe only from an explicit user button press.
- Automatic start is forbidden: no launch-time, setup-time, background, or
  React-effect authentication.
- Required visible states: `pre-auth`, `auth-in-progress`, `success`,
  `cancelled`, `failed`, `deleted`.
- Use the real iOS system authentication sheet.
- Do not imitate biometric UI; do not overlay app-owned chrome on the OS sheet.
- Function only as an app-owned pre-auth / post-auth surface around the trusted
  system exception.
- The harness proves probe plumbing only; it is **not** evidence that the
  production unlock UI is complete.

`KeychainProbePanel` is not created by this documentation amendment.

##### Info.plist source-of-truth

Future implementation must inject usage description via this path only:

- source plist: `apps/desktop/src-tauri/Info.ios.plist`
- Tauri config: `apps/desktop/src-tauri/tauri.ios.conf.json`
- setting: `bundle.iOS.infoPlist = "Info.ios.plist"`
- final verification target: `PKB.app/Info.plist` must contain the exact
  approved `NSFaceIDUsageDescription` string

Forbidden: editing `gen/apple/project.yml`, editing
`gen/apple/pkb-desktop.xcodeproj/project.pbxproj`, hand-editing generated
Info.plist files, running `xcodegen`, or running `tauri ios init` to satisfy
this contract. Creating `Info.ios.plist` or changing `tauri.ios.conf.json` is
out of scope for this documentation amendment.

##### Approved typed `objc2` architecture (future, not yet added)

A later, separately authorized implementation may add the following exact-pin,
Apple-target-only optional dependencies under the existing `secure-vault`
feature:

```toml
objc2 = "=0.6.4"
objc2-security = "=0.3.2"
objc2-local-authentication = "=0.3.2"
objc2-foundation = "=0.3.2"
objc2-core-foundation = "=0.3.2"
```

The Keychain probe must use one `objc2` type system end to end. The future
implementation must remove the `security-framework` path rather than mixing it
with `objc2-security`; the final `secure-vault` dependency graph must not retain
both Keychain binding stacks.

Integration and ownership rules:

- create one `LAContext` and keep it in `objc2::rc::Retained`;
- set the approved exact `localizedReason`;
- construct the heterogeneous query with Foundation collection types, including
  the same `LAContext` under `kSecUseAuthenticationContext`;
- use the public typed `NSDictionary` / `CFDictionary` bridge exposed through
  `objc2-core-foundation` when calling the `objc2-security` SecItem API;
- rely on Foundation collection retention and normal `Retained` / collection
  drop behavior; do not force an ownership transfer;
- never use raw-pointer casts, `Retained::into_raw` / `from_raw`, Core Foundation
  Create/Get-rule wrapping, `transmute`, `mem::forget`, or `ManuallyDrop` to
  bridge the context;
- never use deprecated `kSecUseOperationPrompt`;
- keep every dependency and implementation path inside the Apple-target and
  `secure-vault` feature boundary.

The direct `objc2-core-foundation` dependency is required so the public typed
`CFDictionary` bridge can be named at the `SecItemCopyMatching` call site.
Recording these pins does **not** authorize their addition to the main
repository or prove final-app link success.

##### Fixed Rust toolchain for Apple gates

All iOS type checks and builds for this workstream must select Rust toolchain
`1.96.1` explicitly. The canonical simulator type-check form is:

```bash
cargo +1.96.1 check --offline --target aarch64-apple-ios-sim
```

Tauri or Xcode build entry points must likewise run with toolchain `1.96.1`
selected explicitly; do not rely on the mutable default `stable` toolchain.

##### Mandatory ownership / link feasibility gate (before probe body)

Before implementing the Keychain probe body, UI, or DB work, the gate must prove
all of the following with primary-source evidence and a final iOS link:

1. `LAContext` construction;
2. setting the exact approved `localizedReason`;
3. injecting that same context into the Keychain query;
4. Rust ownership / retain / release justification from primary sources;
5. iOS final link success for the chosen bridge.

Hard stops for that gate: undocumented raw-pointer casts, ownership-unknown
bridges, guessed retain/release, `unwrap` / `expect` / `panic!`,
`kSecUseOperationPrompt`, unproven FFI, or any security-weakening fallback.
The isolated audit has proven items 1 through 4 and passed host plus
`aarch64-apple-ios-sim` type checking with toolchain `1.96.1`. A separately
authorized, minimal main-repository integration must still add the approved
dependencies, replace the old binding path, and pass the final Tauri iOS link
before item 5 can be marked complete. Until that remaining link gate passes, do
not implement the Keychain probe body, production UI, or DB persistence.

##### Evidence separation

Canonicalized by this ruling:

- exact prompt strings;
- `KeychainProbePanel` harness contract;
- Info.plist source-of-truth path;
- the five approved exact dependency pins and single-`objc2` architecture;
- the typed heterogeneous dictionary and `CFDictionary` bridge direction;
- retain/release rules that forbid raw ownership transfer;
- fixed Rust toolchain `1.96.1` for Apple gates;
- isolated host and iOS-simulator ownership/type feasibility for the typed
  bridge.

Still unproven after this documentation amendment:

- dependency addition and replacement of `security-framework` in the main
  repository;
- final Tauri iOS link after adding the approved `objc2-*` dependencies;
- simulator or physical-device `userPresence` runtime;
- cancellation runtime;
- Keychain round-trip / cleanup / zeroization runtime;
- SQLCipher persistent DB / wrong-key / encrypted-at-rest;
- Blueprint-wide Phase 0 completion.

### 4.3 SQLCipher smoke test

In an isolated app container:

1. build and final-link the Tauri iOS app with `secure-vault`;
2. create and key a throwaway SQLCipher DB before any query;
3. create one table, insert a known marker, close, reopen, and read it;
4. prove a wrong key fails and does not alter the file;
5. prove `cipher_version`, compile options, and cipher integrity checks pass;
6. prove the file header is not `SQLite format 3` and scan DB/WAL/SHM/temp files
   for the known plaintext marker;
7. prove the container files are excluded from backup and carry the selected
   Data Protection class;
8. background/foreground the app and prove the connection closes, key bytes are
   zeroized, and re-entry requires OS authentication;
9. run `cargo check` default, `cargo check --features secure-vault`, Rust tests,
   TypeScript checks, and `npm run tauri:ios-dev -- --features secure-vault`.

### 4.4 Phase 0 acceptance and stop conditions

GO requires all of the following evidence:

- final iOS `BUILD SUCCEEDED` and successful launch;
- physical-device Keychain user-presence round trip;
- verified SQLCipher identity and encrypted-at-rest smoke test;
- no duplicate SQLite provider and no OpenSSL;
- no plaintext marker in DB sidecars or temp paths;
- default desktop non-regression;
- only the approved feature-gated files changed.

Any missing symbol, duplicate SQLite provider, unverified Keychain attribute,
wrong-key mutation, plaintext artifact, crash, or `unwrap`/`expect` in the probe
is a HARD STOP. Do not work around a failed gate by weakening accessibility,
removing user presence, switching to system SQLite, or passing a key through JS.

## 5. Phase 1 — Rust vault executor and migrations

### 5.1 Single-connection “pool”

Implement `VaultHandle` as a command sender to one dedicated worker thread. The
worker creates, uses, and drops one `rusqlite::Connection`; no connection guard
crosses the worker boundary. The queue is bounded and every request carries a
reply channel and cancellation/deadline metadata.

Public state is a closed enum:

```text
Unprovisioned | Locked | Unlocking | Unlocked | Locking |
RecoveryRequired | OrphanedKey | Quarantined | Unavailable
```

State transitions are reducer-like and tested. Lock/background requests stop
accepting writes, finish or roll back the active transaction, checkpoint/close
the DB, zeroize the key, and only then publish `Locked`.

### 5.2 Migration discipline

- Migrations are immutable Rust assets with sequential versions and digests.
- Use `PRAGMA user_version` plus a migration-receipt table containing the app,
  SQLCipher, compile-option, schema, and migration digests.
- Apply each migration in an immediate transaction with bounded execution.
- Unknown/newer schema, missing migration, digest mismatch, or failed integrity
  check quarantines the vault; no downgrade, auto-repair, or empty replacement.
- Crash-test every transaction boundary and reopen with the same key.
- Initial schema reserves the roadmap tables: `schema_meta`, `vault_meta`,
  `canonical_record`, `journal_event`, `pending_signed_event`, `import_receipt`,
  and `recovery_receipt`.

### 5.3 Key and lifecycle errors

Map platform/database errors to stable codes such as `AUTH_CANCELLED`,
`KEY_MISSING_WITH_DB`, `KEYCHAIN_UNAVAILABLE`, `WRONG_KEY_OR_CORRUPT`,
`UNSUPPORTED_SCHEMA`, and `VAULT_QUARANTINED`. Error strings contain no path,
SQL, key material, query payload, or Keychain query dump.

## 6. Phase 2 — Kakeibo repository and canonical journal

### 6.1 Domain boundary

Move the canonical persistence type to `domain/kakeibo.rs`. The M5
`KakeiboEntryV1` remains an extraction DTO and converts explicitly into the
domain type. Literal `"unknown"` values become absence/unknown state in the DB;
they are not stored as fabricated facts. Preserve bounded provenance such as
the original user input and extraction task/schema version without storing raw
model internals.

Amounts are stored as checked 64-bit integer minor units. Dates are either a
validated ISO date or NULL. All text and payload sizes have hard upper bounds.
No repository method accepts arbitrary SQL, table names, column names, or
PRAGMAs.

### 6.2 Closed repository operations

Minimum API:

```text
save_kakeibo(validated_entry, provenance, idempotency_key)
get_kakeibo(record_id)
list_kakeibo(bounded_page_request)
delete_kakeibo(record_id, expected_version)
```

Every mutation validates again inside the worker and commits the materialized
record and canonical journal event atomically. Idempotency keys prevent duplicate
saves after response loss. Optimistic version checks prevent silent lost updates.

### 6.3 Signed journal and Keychain anchor

Implement the roadmap's DB-first reservation protocol: durable pending event,
narrow DeviceSigner operation, signature verification, final DB transaction,
then two-slot Keychain anchor finalization. The signer is not a generic
sign-bytes oracle. Prefer a Secure Enclave P-256 key for journal signing; this is
separate from the SQLCipher key.

Crash tests cover every reservation, signature, final transaction, and anchor
boundary. Sequence gaps, same-sequence/different-hash, invalid signatures, and
Keychain-ahead-of-DB states quarantine the vault. M3 only guarantees DB-only
rollback/fork detection described by the roadmap, not coherent rollback of both
DB and Keychain.

## 7. Phase 3 — Tauri and frontend integration

### 7.1 Tauri commands

Register only typed commands:

```text
vault_status()
vault_provision()
vault_unlock()
vault_lock()
kakeibo_save(validated_entry, idempotency_key)
kakeibo_list(cursor, limit)
```

`vault_unlock` triggers native Keychain access internally and returns public
status only. No command accepts or returns a password, SQLCipher key, Keychain
blob, SQL, PRAGMA, database path, or arbitrary signer payload.

### 7.2 Pure frontend state and trusted ceremony

Add a pure `vaultReducer` with explicit effects/events for status refresh,
provision, unlock, lock, and save. React remains a Dumb View. The app shows a
native/system-owned Face ID/Touch ID/passcode ceremony and never imitates it in
HTML/CSS.

Extend the existing M5 `ExtractionSink` seam with a vault-backed sink. Preserve
clipboard export. The save sink accepts only `event.validated` data from the
M5 extraction trust boundary, reports pending/success/error deterministically,
and never parses streamed model text or writes directly to storage.

The UI must distinguish `Locked`, user-cancelled authentication,
`RecoveryRequired`, and `Quarantined`. It must not offer “reset” as an implicit
response to a missing key when a DB exists.

### 7.3 Phase 3 gates

- TypeScript typecheck and dependency-free reducer tests pass.
- Default desktop behavior and byte-level boundary tests remain unchanged.
- iOS simulator proves command/event plumbing; physical device proves trusted
  authentication and lifecycle locking.
- Save/retry is idempotent and successful records survive restart.
- Cancelled authentication and backgrounding never leak or persist key bytes.
- No DB save is possible from raw/unvalidated LLM output.

## 8. Verification matrix and release blockers

| Area | Required evidence |
|---|---|
| Build | default Cargo GREEN; secure-vault Cargo GREEN; final iOS link/deploy GREEN |
| Keychain | exact accessibility, user-presence, non-sync, Data Protection, cancellation, physical device |
| Encryption | SQLCipher version/options receipt, wrong-key failure, integrity pass, encrypted header, plaintext scan |
| Ownership | one SQLite provider and one connection owner; no JS/Python/native opening vault |
| Lifecycle | explicit lock, background, protected-data loss, process kill, and foreground re-authentication |
| Migration | clean install, sequential upgrades, crash boundaries, newer-schema rejection, digest mismatch quarantine |
| Repository | validation, bounds, idempotency, concurrency ordering, rollback, journal/materialized atomicity |
| Recovery | missing-key-with-DB is fail-closed; no false recovery claim before M6 |
| Backup | DB, WAL, SHM, staging, derived data, and model paths excluded and verified in archive drill |
| Leakage | logs/crashes/IPC/UserDefaults/WebView contain no key, SQL, protected payload, or plaintext DB marker |

Release is blocked until the G0-D license/SBOM/export-compliance review is
complete. Do not guess `ITSAppUsesNonExemptEncryption`; record the owner and
decision evidence separately.

## 9. Explicit non-goals and prohibitions

- No network, cloud key escrow, iCloud Keychain synchronization, or model-backed
  recovery.
- No app password for routine device-vault unlock.
- No SQLCipher key in Secure Enclave, JavaScript, environment variables,
  UserDefaults, config files, source code, or logs.
- No `unwrap`, `expect`, panic-based control flow, silent empty-DB recreation, or
  catch-and-continue after integrity failures.
- No arbitrary SQL/PRAGMA IPC, JavaScript database plugin, multi-connection
  pool, or second SQLite owner.
- No automatic deletion/reset of an unreadable vault.
- No recovery/archive implementation before the M6 format and custody ADR.

## 10. Primary references

- Apple, Keychain accessibility:
  <https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly>
- Apple, restricting Keychain item accessibility:
  <https://developer.apple.com/documentation/security/restricting-keychain-item-accessibility>
- Apple, Secure Enclave key protection:
  <https://developer.apple.com/documentation/security/protecting-keys-with-the-secure-enclave>
- Apple, managing user secrets with Keychain:
  <https://developer.apple.com/documentation/security/using-the-keychain-to-manage-user-secrets>
- `security-framework` 3.7.0 documentation:
  <https://docs.rs/crate/security-framework/3.7.0>
- `rusqlite` documentation:
  <https://docs.rs/crate/rusqlite/latest>
- SQLCipher API:
  <https://www.zetetic.net/sqlcipher/sqlcipher-api/>
- SQLCipher key-material guidance:
  <https://www.zetetic.net/sqlcipher/database-key-material/>
- SQLCipher design:
  <https://www.zetetic.net/sqlcipher/design/>
- Project roadmap: `docs/ROADMAP_IOS_TAURI_V2.md`, Workstream C and G0-D.
