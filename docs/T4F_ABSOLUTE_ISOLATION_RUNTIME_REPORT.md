# T4-F-1 / T4-F-2 完遂報告 — 計測装置とその陽性対照

- **相位**: T4-F-1 + T4-F-2（設計案 `docs/T4F_ABSOLUTE_ISOLATION_RUNTIME_PLAN.md`）
- **枝**: `feature/t4f-absolute-isolation-runtime`
- **HEAD**: `f892e558d5349304b8862357650bb177a39fde0f`
- **base**: `6cf99d9922b4e76431e9700859b1d9e8d935e422`
- **採取時刻 (UTC)**: `2026-08-04T15:12:58Z`
- **証跡ディレクトリ**: `/tmp/t4f-evidence`
- **プロファイル失効（指示書 1.4）**: `2026-08-11T10:40:02Z`

本フェーズの成果物は**実行結果（実機沈黙の 0）ではなく、指揮官が sudo で回せる計測装置**である（C-3）。実装担当は `tcpdump`/`rvictl` の特権操作を実行していない。

---

## I-1 — 装置の順序（陽性対照 → 本計測 → 撤去）

`scripts/t4f_capture_session.sh`（digest `cc3427f2118fff5802ff08d746fd6d6b2327a0b9332a3a0cc1ff53a8b531f5bf`）:

| Phase | 内容 | 接地 |
|---|---|---|
| 0 | USB / 無線 ON を指揮官に指示。`devicectl` 出力をファイル化（申告のみで進めない） | transport ヒントを記録 |
| 1 | `rvictl -s` 後、**`ifconfig -l` で rvi\* 実在を確認**（`rvictl -l` は使わない） | 指示書 1.3 |
| 2 | 陽性対照。失敗なら本計測へ進まず exit 1 | I-2 |
| 3 | 本計測（Test A のみ）。`T4F_TEST=B` は exit 2 で拒否 | C-4 |
| 4 | `rvictl -x` + rvi\* 不在確認 | 撤去 |

Harness 順序実証（sudo 無し）: `/tmp/t4f-evidence/capture_harness_pass_stdout.txt` digest `d9bfab8ab3ad95a44ead4c5e482ceb9af6f85e3dcd053c78967a8d089bfdfabc`。

---

## I-3 — 陽性対照の設計判断（候補・採否・理由）

### 採用: Safari（Coraxis 以外）で `http://example.com/` を開く

| 条件 | 充足 |
|---|---|
| 対象アプリ以外 | ブラウザは Coraxis ではない（装置が明示指示） |
| 端末側から出る | rvi0 に乗るのはデバイス発トラフィック |
| 再現性 | 同一 URL・同一手順をセッション内で繰り返せる |
| プライバシー | 証跡は **packet_lines / unique_ipv4_endpoints の件数のみ**。対照 pcap は件数取得後に削除（C-7） |

無線 liveness の自動検証もこの対照に兼ねる: **対照パケットが 1 本でも乗れば、その瞬間インターフェースと無線経路は生きている**。機内モードでは対照が発火しない → 本計測へ進めない（空虚な緑を構造的に拒否）。

### 却下した候補

| 候補 | 却下理由 |
|---|---|
| Mac 上の `curl` / ブラウザ | rvi0 に乗らない（Mac 側トラフィック） |
| Coraxis 自身に通信させる | 本末転倒。装置の健全性と被測定物が同一になる |
| iCloud / Mail / Photo 同期の偶発トラフィック | 再現性が無い。内容が私的通信になり C-7 に抵触しやすい |
| Mac→端末への ping | 端末発ではない。egress 沈黙の対照として入力クラスが違う |
| Personal Hotspot の起動 | 侵襲が大きく、対照として過剰 |

### port-scan 陽性対照（I-5）

採用: **Mac 側 `127.0.0.1` に一時 listen**し、nmap が `open` を返すことを先に実証。指示書 3.2 は「端末側 or Mac 側」を明示的に許容する。

実測（`2026-08-04T15:07:34Z` セッション）: `127.0.0.1:62069/tcp open`（`/tmp/t4f-evidence/positive_control_nmap.txt` digest `d023f1e77ae9a98fdfaf4a964474814a4764b998e8a635edc9d3298c6c3b4c3d`）。

---

## I-4 — 「app-owned」判定基準と切り分け不能範囲

装置は計測前に `app_owned_criterion.txt` を必ず書く。要約:

1. **rvi0 + tcpdump に PID / bundle id は載らない。** プロセス単位の切り分けはできない。
2. 報告してよい数値は、**同一セッションで陽性対照が発火したあとの device-wide packet/byte 系**に限る。
3. **`app-owned network bytes = 0` と書いてはならない。** この装置だけでは **NOT SEPARABLE**。
4. app-owned を 0 と主張できるのは、別の帰属手段（例: Instruments の process filter）を**それ自体の陽性対照付きで**導入したあとに限る。本フェーズの射程外。

「切り分けられなかったので 0」は禁止（C-6）。沈黙の device-wide 件数すら、対照未発火なら報告禁止（C-5）。

---

## I-2 — 陽性対照失敗で本計測へ進まないこと（変異ドリル）

`scripts/t4f_capture_positive_control_mutation_drill.sh` digest `7084b5c48fa7c7626e7750140c411d4667be924587c5de5ab391a080019f5839`:

| Step | 結果 |
|---|---|
| intact + CONTROL=fail | exit=1 / `measurement_summary.txt` **ABSENT** |
| gate 破壊（fail を無視） | artifact **PRESENT**（ゲートが荷重を支えていることの証明） |
| 復元 + fail | 再び ABSENT |
| intact + pass | PRESENT（順序の正系） |
| `T4F_TEST=B` | exit=2（機内モード相当キャプチャ拒否） |

stdout digest: `cf3ae861d52179d41cf8115ae330e63c9f856b96b68fefbc494d861a5097f88d` → `DRILL[t4f_capture_positive_control]: PASS`

port-scan 同型ドリル: `scripts/t4f_port_scan_positive_control_mutation_drill.sh` digest `d29a34fb360c72b4846610c898116701ed40b462d17c95f00fdc6fbf47329628` → `DRILL[t4f_port_scan_positive_control]: PASS`（stdout `2c3e0769e666966bd9cb126b3203433bf386d417db1812306516701bc0070db5`）。

---

## I-5 / I-6 — port scan 装置と範囲の明示

`scripts/t4f_port_scan.sh` digest `c339d94f8fc8755d208a329ca5f98d95183de22f92be3c1ee11dced83471b4b4`。

| `T4F_PORT_SCOPE` | 意味 |
|---|---|
| `top1000`（既定） | `nmap --top-ports 1000` — **全 65535 ではない** |
| `full` | tcp `1-65535` |
| `custom:SPEC` | 明示仕様 |

既定スコープを証跡に固定: `/tmp/t4f-evidence/scan_scope_default.txt` digest `e61dfc2b0b65645ff4438edcd0864000cac28537e233d44fc612d6df2179feaf`。  
ライブ対照セッションの scope 行: `/tmp/t4f-evidence/portscan_live_scope.txt` digest `de495ca21f57d8c30ffabb970811982471b8e2cf9d66cba11da7e388d67ab153`（`stated_at_utc=2026-08-04T15:07:34Z`）。

**本フェーズで実機 IP への device scan は未実施**（`T4F_SKIP_DEVICE=1`）。対照のみ実証。範囲を語れない緑は出していない。

---

## I-7 — 物理作業の指示と状態の自動検証

装置が出す指示（黙って待たない）:

1. USB 接続を確認 → Enter。`devicectl list` を保存し wired/USB ヒントを探す（無い場合は WARN。最終接地は rvi 実在）。
2. **試験 A: 無線 ON**（機内モード禁止）→ Enter。liveness は陽性対照パケットで証明。
3. 対照: Safari で control URL → Enter。
4. 本計測ウィンドウ → Enter。
5. 撤去後 `ifconfig -l` に rvi\* が無いこと。

**試験 B（機内モード）でパケットキャプチャする経路は装置が拒否する**（exit 2）。混同しない。

---

## ツール在庫の再測（実装担当・sudo 無し）

`/tmp/t4f-evidence/tool_inventory.txt` digest `1cd49994b1eb288abc0f104ab8ebfcb72c8663c4b7fd44a7b76774d02a3f15c6`（`measured_utc=2026-08-04T15:12:58Z`）:

| 項目 | 実測 |
|---|---|
| nmap | 7.99（`/opt/homebrew/bin/nmap`） |
| tcpdump | 4.99.1 Apple 153 |
| rvictl | 在 |
| `/dev/bpf0` | `crw------- root wheel` |
| `sudo -n true` | exit 1 / `sudo: a password is required` |
| rvi\* | NONE（未起動。正当 — 実装担当は上げない） |

---

## 指揮官が実行するコマンド（本計測は未実施）

USB 接続・無線 ON のうえで:

```bash
sudo T4F_UDID=00008150-000269103CF0401C \
  T4F_EVIDENCE_DIR=/tmp/t4f-evidence/capture-live \
  bash scripts/t4f_capture_session.sh
```

port scan（対照後に実機）:

```bash
T4F_DEVICE_IP=<設定に表示される端末 IP> \
  T4F_PORT_SCOPE=top1000 \
  T4F_EVIDENCE_DIR=/tmp/t4f-evidence/portscan-live-device \
  bash scripts/t4f_port_scan.sh
```

全ポートを見たと言うなら、そのセッションで `T4F_PORT_SCOPE=full` を明示し、`scan_scope.txt` を証跡に残せ。

---

## I-8 — CI 16/16

既存封緘ゲート・workflow・`ios_archive_scan.sh` / `gguf_three_point_sha_gate.sh` は **無改変**。本フェーズは `scripts/t4f_*.sh` と文書のみ。ruleset 必須化・push は未実施（C-8）。

---

## I-9 — 実施しなかったこと・確かめなかったこと

1. **実機 rvi0 上のライブ tcpdump**（sudo が要る — C-3。装置のみ提供）。
2. **試験 A の Coraxis 本計測完走**（T4-F-3）。装置の Phase 3 枠のみ。
3. **試験 B 機内モード機能完走**（T4-F-4）。キャプチャ経路は意図的に拒否のみ。
4. **実機 IP への nmap device scan**（対照のみ。`T4F_DEVICE_IP` 未投入）。
5. **app-owned = 0 の主張** — NOT SEPARABLE と定義した（測っていないものを 0 と書かない）。
6. commit / push（C-8）。
7. symbol/capability ランタイム走査（T4-F-5）。
8. `devicectl` の安定列挙 — 本環境では CoreDevice XPC がタイムアウトすることがあり、USB 接地の補助に過ぎない（主接地は ifconfig の rvi\*）。

---

## I-10 / 成果物

| 種別 | パス | digest (sha256) |
|---|---|---|
| キャプチャ装置 | `scripts/t4f_capture_session.sh` | `cc3427f2118fff5802ff08d746fd6d6b2327a0b9332a3a0cc1ff53a8b531f5bf` |
| port-scan 装置 | `scripts/t4f_port_scan.sh` | `c339d94f8fc8755d208a329ca5f98d95183de22f92be3c1ee11dced83471b4b4` |
| キャプチャ変異ドリル | `scripts/t4f_capture_positive_control_mutation_drill.sh` | `7084b5c48fa7c7626e7750140c411d4667be924587c5de5ab391a080019f5839` |
| port-scan 変異ドリル | `scripts/t4f_port_scan_positive_control_mutation_drill.sh` | `d29a34fb360c72b4846610c898116701ed40b462d17c95f00fdc6fbf47329628` |
| 本報告 | `docs/T4F_ABSOLUTE_ISOLATION_RUNTIME_REPORT.md` | （verify 後に提出メッセージへ） |

検証:

```bash
bash scripts/t4f_capture_positive_control_mutation_drill.sh
bash scripts/t4f_port_scan_positive_control_mutation_drill.sh
T4F_SKIP_DEVICE=1 bash scripts/t4f_port_scan.sh
bash scripts/verify_report.sh docs/T4F_ABSOLUTE_ISOLATION_RUNTIME_REPORT.md /tmp/t4f-evidence
```
