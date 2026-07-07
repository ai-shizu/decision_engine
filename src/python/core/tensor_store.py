#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/tensor_store.py — 時系列直交結合マトリクス (Target Echo / E1)
==================================================================================
DailyContext (日記/相談/家計簿/予定/LINE) を同一の日次格子へ射影した
密行列 (日数 × 32特徴量 + 欠測マスク) を PKBTEN01 として mmap ゼロコピー
共有する。設計の核心は docs/SPEC_ECHO_GENESIS.md §1〜§5。

C++ 側の唯一の対応物は src/cpp/search_engine.cpp の TensorHeader/TensorRow。
変更は両方同時 + magic バージョン更新 (PKBVEC01/PKBSCR01 と同規則)。

【scratch (PKBSCR01) と逆の規律 — SPEC §5.1.1 W-5】
scratch は in-place 更新が正だが、本テンソルは in-place 更新を禁止する。
更新は必ず全再構築 + tmp → os.replace のみ。再構築前に必ず
release_tensor_mapping() で自プロセス内の open ハンドルを解放すること
(I-9 の適用: remap 前にハンドルを解放してから書く)。

欠測は valid_mask のみで表現し、NaN は格納しない (I-18)。値スロットは
欠測時も 0.0 を書く — 「0円」と「未記録」の区別はマスクが担う。
"""

from __future__ import annotations

import hashlib
import json
import mmap
import os
import struct
from datetime import date as _date, timedelta
from pathlib import Path

import numpy as np

# 呼び出し側 (oracle.py/facade.py) が `tensor_store.TENSOR_GLOBAL_BIN` /
# `tensor_store.tensor_dyad_bin` として参照する再エクスポート。
from .paths import TENSOR_GLOBAL_BIN, tensor_dyad_bin  # noqa: F401

# ---------------------------------------------------------------- レイアウト定義 (凍結)
TENSOR_MAGIC = b"PKBTEN01"
KTEN_FEAT = 32

_HEADER_FMT = "<8sIIIIiIQ24x"   # magic, version, n_rows, n_features, row_stride,
                               # epoch_day, flags, content_hash64, reserved[24]
_ROW_FMT = "<iI32f"            # day_index, valid_mask, f[32]

HEADER_SIZE = struct.calcsize(_HEADER_FMT)
ROW_SIZE = struct.calcsize(_ROW_FMT)
# C++ 側 static_assert と対をなす相互検証 (W-1: "<" 欠落によるパディング混入の検出)
assert HEADER_SIZE == 64, HEADER_SIZE
assert ROW_SIZE == 136, ROW_SIZE

# W-1: numpy dtype も明示リトルエンディアン固定 (native 表記は現行全ターゲットが
# LE のため「たまたま」一致するが、規約はあくまで明示 LE)。
ROW_DTYPE = np.dtype([("day", "<i4"), ("mask", "<u4"), ("f", "<f4", (KTEN_FEAT,))])
assert ROW_DTYPE.itemsize == ROW_SIZE, ROW_DTYPE.itemsize

FLAG_DYAD_SCOPE = 1 << 0

# ---------------------------------------------------------------- 特徴量レジストリ (§5.2)
# レーン番号は永久凍結。付番の再利用禁止 (拡張は Architect's Note を要する)。
FEATURES: dict[str, int] = {
    "diary_chars": 0,
    "abstract_idx": 1,
    "guilt_idx": 2,
    "productivity_idx": 3,
    "consult_count": 4,
    "spend_total": 5,
    "spend_hedonic": 6,
    "spend_invest": 7,
    "cal_event_count": 8,
    "cal_private_hours": 9,
    "cal_switch_count": 10,
    "line_out_msgs": 11,
    "line_in_msgs": 12,
    "line_out_chars": 13,
    "line_reply_med_min": 14,
    "line_initiations": 15,
    "line_night_out": 16,
    "friction_events": 17,
    "task_declared": 18,
    "task_executed": 19,
    "github_commits": 20,    # importer 未実装 (Echo スコープ外)。常に mask=0
    "leetcode_solved": 21,   # 同上
}
N_ACTIVE_FEATURES = len(FEATURES)   # 22。lane 22-31 は reserved (常に mask=0)

# calendar.json は開始時刻 (time) のみを持ち終了時刻を持たない
# (calendar_manager.py 参照)。「合計時間」の真値は現行データモデルでは測定不能
# なため、1 件あたりの名目値で近似する。
# 【実装時の発見 — SPEC は「合計時間」を指示するが、基盤データに duration
#  フィールドが存在しない。将来 calendar.json に終了時刻が追加されたら、
#  この定数近似を実測値の合算に置き換えること。】
PRIVATE_EVENT_NOMINAL_HOURS = 1.5

_EPOCH = _date(1970, 1, 1)


class TensorStoreError(RuntimeError):
    """テンソルの読み込み/構築の失敗。scratch と異なり in-place 更新は一切
    許されない (SPEC §5.1.1 W-5) — OS 依存の失敗を待たずここへ集約する。"""


# ---------------------------------------------------------------- ハンドル registry (T-14)
# rebuild-under-handle (罠 T-14): view 保持中の rebuild は未定義動作になり得る。
# 自プロセス内のハンドルは build_tensor() 実行前に必ず close する。
_OPEN: dict[str, "TensorStore"] = {}


def release_tensor_mapping(path: Path) -> None:
    """再構築の直前に必ず呼ぶ (I-9)。対象パスを保持する自プロセス内の
    TensorStore を close する。他プロセスの残存ハンドルはここでは検出できない
    — その場合 build_tensor() の os.replace が OSError を出し、
    TensorStoreError として先取りする (SPEC §5.1.1 W-5)。"""
    key = str(Path(path).resolve()) if Path(path).exists() else str(Path(path))
    store = _OPEN.pop(key, None)
    if store is not None:
        store.close()


# ---------------------------------------------------------------- 鮮度判定
def content_hash64(daily: list[dict]) -> int:
    """入力スナップショットの鮮度判定鍵。blake2b 先頭 8 バイトを符号なし64bit整数へ
    (Charlie の content_hash と同じ正規化 JSON 規約)。"""
    blob = json.dumps(daily, ensure_ascii=False, sort_keys=True).encode("utf-8")
    digest = hashlib.blake2b(blob, digest_size=8).digest()
    return int.from_bytes(digest, "little")


def _to_epoch_day(d: _date) -> int:
    return (d - _EPOCH).days


def _from_epoch_day(n: int) -> _date:
    # T-17: 日数差分は date オブジェクトの演算のみで行う。epoch 秒/86400 は
    # DST/うるう秒で暦日とずれるため使わない。
    return _EPOCH + timedelta(days=n)


# ---------------------------------------------------------------- 読み込み
class TensorStore:
    """PKBTEN01 の読み取り専用 mmap ラッパー。書き込みは build_tensor() のみ。"""

    def __init__(self, path: Path):
        self.path = Path(path)
        self._closed = False
        size = self.path.stat().st_size
        if size < HEADER_SIZE:
            raise TensorStoreError(f"{self.path}: ファイルが header 未満 ({size}B)")

        self._file = open(self.path, "rb")
        self._mm = mmap.mmap(self._file.fileno(), 0, access=mmap.ACCESS_READ)

        header = self._mm[:HEADER_SIZE]
        magic, version, n_rows, n_features, row_stride, epoch_day, flags, chash = \
            struct.unpack(_HEADER_FMT, header)

        if magic != TENSOR_MAGIC:
            self._fail_close(f"{self.path}: magic 不一致 {magic!r}")
        if version != 1:
            self._fail_close(f"{self.path}: 未対応バージョン {version}")
        if row_stride != ROW_SIZE:
            # W-4: row_stride は必ずヘッダから読み、現行コードの前提と照合する。
            # 不一致は「将来フォーマットを現行コードが誤読する」ことを意味し、
            # 読めるところまで読むのではなく即エラーにする (前方互換の生命線)。
            self._fail_close(
                f"{self.path}: row_stride={row_stride} は現行コードの前提 "
                f"({ROW_SIZE}) と不一致 — 前方互換のない書式")
        expected_size = HEADER_SIZE + n_rows * row_stride
        if size != expected_size:
            self._fail_close(
                f"{self.path}: file_size={size} != header 由来の期待値 "
                f"{expected_size} (n_rows={n_rows}) — 切詰め/破損")

        rows = np.frombuffer(self._mm, dtype=ROW_DTYPE, count=n_rows, offset=HEADER_SIZE)
        if rows.flags.writeable:
            # ACCESS_READ の mmap から作った view が writeable になることは
            # ないはずだが、in-place 更新経路の不存在を W-4 として明示検証する。
            self._fail_close(f"{self.path}: rows view が writeable (ACCESS_READ 違反)")
        if not np.array_equal(rows["day"], np.arange(n_rows, dtype=np.int32)):
            self._fail_close(f"{self.path}: day_index が行番号と不一致 (破損/欠落行)")
        if np.isnan(rows["f"]).any():
            self._fail_close(f"{self.path}: NaN 混入 (I-18 違反 — 欠測は mask のみで表現)")
        if n_features < KTEN_FEAT:
            bad_bits = np.uint32((0xFFFFFFFF << n_features) & 0xFFFFFFFF)
            if np.any(rows["mask"] & bad_bits):
                self._fail_close(
                    f"{self.path}: valid_mask に n_features={n_features} 超の"
                    "ビットが立っている (reserved レーンの mask 違反)")

        self.version = version
        self.n_rows = n_rows
        self.n_features = n_features
        self.row_stride = row_stride
        self.epoch_day = epoch_day
        self.flags = flags
        self.content_hash64 = chash
        self._rows = rows
        _OPEN[str(self.path.resolve())] = self

    def _fail_close(self, message: str) -> None:
        try:
            self._mm.close()
        finally:
            self._file.close()
            self._closed = True
        raise TensorStoreError(message)

    def _check_open(self) -> None:
        if self._closed:
            raise TensorStoreError(f"{self.path}: close 済みの TensorStore を使用した")

    @property
    def dates(self) -> tuple[str, str]:
        """(row0 の ISO 日付, 最終行の ISO 日付)。"""
        self._check_open()
        first = _from_epoch_day(self.epoch_day)
        last = _from_epoch_day(self.epoch_day + self.n_rows - 1)
        return first.isoformat(), last.isoformat()

    def window(self, start_iso: str, end_iso: str) -> tuple[np.ndarray, np.ndarray]:
        """[start_iso, end_iso] (両端含む) の (values (D,32) f32, mask (D,32) bool)
        をゼロコピー view として返す。長期保持する場合は呼び出し側が .copy()
        すること (罠 T-14 — view 保持中の rebuild は未定義動作になり得る)。"""
        self._check_open()
        start_idx = _to_epoch_day(_date.fromisoformat(start_iso)) - self.epoch_day
        end_idx = _to_epoch_day(_date.fromisoformat(end_iso)) - self.epoch_day
        start_idx = max(0, start_idx)
        end_idx = min(self.n_rows - 1, end_idx)
        if end_idx < start_idx:
            return (np.zeros((0, KTEN_FEAT), dtype=np.float32),
                    np.zeros((0, KTEN_FEAT), dtype=bool))
        sl = self._rows[start_idx:end_idx + 1]
        values = sl["f"]
        mask = np.zeros((sl.shape[0], KTEN_FEAT), dtype=bool)
        for lane in range(KTEN_FEAT):
            mask[:, lane] = (sl["mask"] & np.uint32(1 << lane)) != 0
        return values, mask

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        # np.frombuffer 由来の self._rows は mmap のバッファを export したまま
        # 保持している。参照を先に手放さないと mmap.close() が
        # BufferError("cannot close exported pointers exist") になる。
        # 呼び出し側が window() の戻り値をまだ保持している場合はそちらの参照が
        # 生き残るため、この解放だけでは救えない — 罠 T-14 の本体であり、
        # 長期保持する側が .copy() する規律でのみ解決できる。
        self._rows = None
        try:
            self._mm.close()
        finally:
            self._file.close()
        _OPEN.pop(str(self.path.resolve()), None)


# ---------------------------------------------------------------- レーン計算ヘルパー
def _event_theme(title: str) -> str | None:
    from .gap_analysis import THEME_TAXONOMY
    for theme, spec in THEME_TAXONOMY.items():
        if any(kw in title for kw in spec["calendar_keywords"]):
            return theme
    return None


def _task_daily_counts(
    daily: list[dict],
) -> tuple[dict[str, int], set[str], dict[str, int], set[str]]:
    """TASK_LEXICON/DECLARE_MARKERS/DONE_MARKERS を再利用した日別の宣言/実行件数。
    gap_analysis.analyze_procrastination() と同じ語彙・同じ判定条件を用いるが、
    遅延計算 (双曲割引) は行わず日別カウントのみを返す (lane 18/19 専用)。

    E1.1 是正 (SPEC Rev.3 §5.10.2): カウント辞書に加えて「その日その観測
    チャネルが実際に存在したか」の日付集合を返す。呼び出し側 (build_tensor) は
    この集合に無い日を mask=0 のままにする — 「その日そのチャネルの記録が
    無かった」を「0件という観測」に化けさせない (I-18 違反の是正)。
    genuine な主観 doc / 実行観測チャネルが 1 つも無い日にまで mask=1 を
    立てると、日記執筆習慣と相関する全レーンとの間に共有欠測パターン由来の
    偽結合が立つ (E2 の帰無分布を汚染する最悪のアーティファクト源だった)。"""
    import re as _re
    from .gap_analysis import (
        DECLARE_MARKERS, DONE_MARKERS, GENUINE_DOC_MIN_WEIGHT, TASK_LEXICON,
        build_subjective_corpus,
    )

    declared: dict[str, int] = {}
    declared_observed: set[str] = set()
    for doc in build_subjective_corpus(daily):
        # 建前人格 (低ウェイト) からは宣言を採らない (憲法5 / gap_analysis §7.1.4)
        if float(doc.get("weight", 1.0)) < GENUINE_DOC_MIN_WEIGHT:
            continue
        # genuine な主観 doc が存在した時点で「宣言 0 件」も観測済みとして正当
        declared_observed.add(doc["date"])
        for _task, kws in TASK_LEXICON.items():
            for kw in kws:
                for m in _re.finditer(_re.escape(kw), doc["text"]):
                    s = max(0, m.start() - 25)
                    snippet = doc["text"][s:m.end() + 30]
                    if (any(dm in snippet for dm in DECLARE_MARKERS)
                            and not any(dn in snippet for dn in DONE_MARKERS)):
                        declared[doc["date"]] = declared.get(doc["date"], 0) + 1

    executed: dict[str, int] = {}
    executed_observed: set[str] = set()
    for dc in daily:
        d = dc["date"]
        has_calendar = bool(dc.get("calendar_events"))
        has_line_self = bool((dc.get("line_self_text") or "").strip())
        has_diary = bool((dc.get("diary_text") or "").strip())
        if has_calendar or has_line_self or has_diary:
            executed_observed.add(d)
        cnt = 0
        for _task, kws in TASK_LEXICON.items():
            for ev in dc.get("calendar_events", []):
                if any(kw in str(ev.get("title", "")) for kw in kws):
                    cnt += 1
            line = dc.get("line_self_text", "") or ""
            if line and any(kw in line for kw in kws) and any(dn in line for dn in DONE_MARKERS):
                cnt += 1
            diary = dc.get("diary_text", "") or ""
            for kw in kws:
                for m in _re.finditer(_re.escape(kw), diary):
                    s = max(0, m.start() - 25)
                    snippet = diary[s:m.end() + 30]
                    if any(dn in snippet for dn in DONE_MARKERS):
                        cnt += 1
        if cnt:
            executed[d] = cnt
    return declared, declared_observed, executed, executed_observed


def _line_daily_aggregates(
    line_messages: list[dict], *,
    group_contacts: set[str] | None = None,
    contact: str | None = None,
) -> tuple[dict[str, dict], tuple[str, str] | None]:
    """LINE 全ログを日別に集計する (lane 11-17 専用)。バースト抽出・摩擦検出は
    line_telemetry.py の既存実装 (DL1) を呼ぶだけで、burst 状態機械を
    ここで再実装しない (「既存関数を再利用できるかは対称性/方向性まで
    確認してから決める」— AI_SKILLS §11.1 の教訓の継承)。

    戻り値の第2要素はログのカバレッジ窓 (最古日, 最新日)。LINE は完全記録の
    ログであるため、この窓の内側にある無通信日は「欠測 (mask=0)」ではなく
    「観測済み 0 (mask=1, value=0)」である — 沈黙そのものが対人テレメトリの
    最重要シグナルであり、それを欠測として脱落させると相関計算が活動日への
    条件付けバイアスを起こす (E1.1 是正。SPEC Rev.3 §5.10.2 の項目3)。
    lane 14 (line_reply_med_min) は返信サンプルが無い日は従来通り mask=0 —
    「中央値が 0」は無意味なので、この鏡像対称化から明示的に除外する。"""
    from datetime import datetime as _dt

    from . import line_telemetry as lt

    group_contacts = group_contacts or set()
    msgs = [m for m in line_messages if m.get("contact") not in group_contacts]
    if contact is not None:
        msgs = [m for m in msgs if m.get("contact") == contact]

    msg_dates = sorted({
        d for m in msgs
        if (d := lt.normalize_date(m.get("date") or "")) is not None
    })
    coverage = (msg_dates[0], msg_dates[-1]) if msg_dates else None

    days: dict[str, dict] = {}

    def slot(d: str) -> dict:
        return days.setdefault(d, {"out_msgs": 0, "in_msgs": 0, "out_chars": 0,
                                    "night_out": 0, "initiations": 0,
                                    "friction": 0, "reply_latencies": []})

    for m in msgs:
        d = lt.normalize_date(m.get("date") or "")
        if not d or not m.get("time"):
            continue
        dt = lt._parse_dt(d, m["time"])
        s = slot(d)
        if m.get("is_self"):
            s["out_msgs"] += 1
            s["out_chars"] += len(str(m.get("text", "")))
            if lt._is_night_arrival(dt):
                s["night_out"] += 1
        else:
            s["in_msgs"] += 1

    by_contact = lt._bursts_by_contact(msgs)
    for _contact_name, bursts in by_contact.items():
        # 1st pass: 本人の自己ベースライン (user_median) — friction 判定専用
        # (DL1 と同一規約: 相手の速度ではなく本人自身の通常返信速度と比較する)
        user_latencies: list[float] = []
        for i in range(1, len(bursts)):
            prev, b = bursts[i - 1], bursts[i]
            if prev["is_self"] != b["is_self"] and b["is_self"]:
                gap_min = (b["start"] - prev["end"]).total_seconds() / 60
                if gap_min <= lt.LATENCY_OUTLIER_HOURS * 60 and not lt._is_night_arrival(prev["end"]):
                    user_latencies.append(gap_min)
        user_median = lt._median(user_latencies)

        # 2nd pass: initiation (会話の開始者) / 日別レイテンシ帰属
        prev_end = None
        for i, b in enumerate(bursts):
            if prev_end is None or b["start"] - prev_end > lt.CONVERSATION_GAP:
                if b["is_self"]:
                    slot(b["start"].date().isoformat())["initiations"] += 1
            if i > 0:
                prev = bursts[i - 1]
                if prev["is_self"] != b["is_self"] and b["is_self"]:
                    gap_min = (b["start"] - prev["end"]).total_seconds() / 60
                    if gap_min <= lt.LATENCY_OUTLIER_HOURS * 60 and not lt._is_night_arrival(prev["end"]):
                        slot(b["start"].date().isoformat())["reply_latencies"].append(gap_min)
            prev_end = b["end"]

        for ev in lt._detect_friction_events(bursts, user_median):
            ev_date = _dt.fromisoformat(ev["at"]).date().isoformat()
            slot(ev_date)["friction"] += 1

    return days, coverage


# ---------------------------------------------------------------- 書き込み
def build_tensor(daily: list[dict], dyads: list | None, out_path: Path,
                 scope: str = "global", alias: str | None = None, *,
                 line_messages: list[dict] | None = None,
                 group_contacts: set[str] | None = None,
                 contact: str | None = None) -> Path:
    """PKBTEN01 の全再構築 (差分機構は作らない — 再構築が安価な派生データに
    LSM 的な差分機構を持ち込むのは過剰設計。SPEC §1.6)。

    line_messages: dyad スコープ以外では省略可 (省略時は lane 11-17 が全日程で
    mask=0 になる)。SPEC の呼び出し規約は daily/dyads のみを要求するが、
    日次 LINE 集計には日付付き生メッセージが不可欠で dyads (履歴全体の集計値)
    には日付分解能がないため、この2つの keyword-only 引数を追加した
    (最小限の後方互換な拡張 — 位置引数のシグネチャは変更していない)。
    """
    out_path = Path(out_path)
    release_tensor_mapping(out_path)   # W-5: 再構築前に自プロセス内ハンドルを解放

    by_date = {dc["date"]: dc for dc in daily}
    if not by_date:
        raise TensorStoreError("daily が空 — テンソルを構築する日が無い")
    dates_sorted = sorted(by_date)
    first_d = _date.fromisoformat(dates_sorted[0])
    last_d = _date.fromisoformat(dates_sorted[-1])
    n_rows = (last_d - first_d).days + 1

    def idx_of(d: _date) -> int:
        return (d - first_d).days

    values = np.zeros((n_rows, KTEN_FEAT), dtype=np.float32)
    valid = np.zeros((n_rows, KTEN_FEAT), dtype=bool)

    def set_lane(day_idx: int, lane_name: str, value: float) -> None:
        lane = FEATURES[lane_name]
        values[day_idx, lane] = float(value)
        valid[day_idx, lane] = True

    from .gap_analysis import (
        ABSTRACT_LEXICON, GUILT_MARKERS, PRIVATE_TIME_KEYWORDS,
        PRODUCTIVITY_MARKERS, THEME_TAXONOMY,
    )

    for date_str, dc in by_date.items():
        i = idx_of(_date.fromisoformat(date_str))
        sources = set(dc.get("sources", []))
        diary = dc.get("diary_text", "") or ""

        if "diary" in sources:
            set_lane(i, "diary_chars", len(diary))
            set_lane(i, "abstract_idx", sum(diary.count(kw) for kw in ABSTRACT_LEXICON))
            set_lane(i, "guilt_idx", sum(diary.count(kw) for kw in GUILT_MARKERS))
            set_lane(i, "productivity_idx", sum(diary.count(kw) for kw in PRODUCTIVITY_MARKERS))

        if "consultation" in sources:
            genuine = sum(1 for c in dc.get("consultations", [])
                         if not c.get("is_simulated_persona"))
            set_lane(i, "consult_count", genuine)

        if "finance" in sources and scope != "dyad":
            # dyad スコープの lane 5-7 は spend_tagged (dyad 支出タグ) に読み替える
            # 設計だが、タグ規則の config がまだ存在しない (Echo スコープ外) ため
            # dyad テンソルではこの3レーンを常に mask=0 とする。
            expense = [tx for tx in dc.get("transactions", []) if tx.get("type") == "expense"]
            total = sum(int(tx.get("amount", 0)) for tx in expense)
            hedonic = sum(
                int(tx.get("amount", 0)) for tx in expense
                if any(kw in str(tx.get("category", ""))
                      for kw in THEME_TAXONOMY["娯楽・消費"]["spend_categories"]))
            invest = sum(
                int(tx.get("amount", 0)) for tx in expense
                if any(kw in str(tx.get("category", ""))
                      for kw in THEME_TAXONOMY["学習・自己投資"]["spend_categories"]))
            set_lane(i, "spend_total", total)
            set_lane(i, "spend_hedonic", hedonic)
            set_lane(i, "spend_invest", invest)

        if "calendar" in sources:
            events = dc.get("calendar_events", [])
            set_lane(i, "cal_event_count", len(events))
            private_n = sum(1 for ev in events
                           if any(kw in str(ev.get("title", "")) for kw in PRIVATE_TIME_KEYWORDS))
            set_lane(i, "cal_private_hours", private_n * PRIVATE_EVENT_NOMINAL_HOURS)
            switches = 0
            prev_theme = None
            for ev in events:   # data_merger の規約で time 昇順に既にソート済み
                theme = _event_theme(str(ev.get("title", "")))
                if prev_theme is not None and theme != prev_theme:
                    switches += 1
                prev_theme = theme
            set_lane(i, "cal_switch_count", switches)

    # E1.1 是正 (SPEC Rev.3 §5.10.2-1,2): mask=1 は「その日その観測チャネルが
    # 実際に存在した」日のみ。全日付へ無条件に mask=1 を立てると、「日記を
    # 書かなかった日」が「宣言0件という観測」に化け、日記執筆習慣と相関する
    # 全レーンとの間に共有欠測パターン由来の偽結合を生む (E2 の帰無分布汚染)。
    declared, declared_observed, executed, executed_observed = _task_daily_counts(daily)
    for date_str in by_date:
        i = idx_of(_date.fromisoformat(date_str))
        if date_str in declared_observed:
            set_lane(i, "task_declared", declared.get(date_str, 0))
        if date_str in executed_observed:
            set_lane(i, "task_executed", executed.get(date_str, 0))

    # E1.1 是正 (SPEC Rev.3 §5.10.2-3): LINE は完全記録のログであるため、
    # ログのカバレッジ窓 [最古日, 最新日] 内の無通信日は「欠測」ではなく
    # 「観測済み0」(mask=1, value=0.0) とする。窓の外側は依然として mask=0
    # (ログが存在しない期間について沈黙かどうかは判定できない)。
    # lane 14 (line_reply_med_min) だけは鏡像対称化から除外する — 返信サンプル
    # が無い日の「中央値」は定義できない値であり、0 で埋めると意味不明瞭になる。
    _SILENT_ZERO_LANES = ("line_out_msgs", "line_in_msgs", "line_out_chars",
                         "line_night_out", "line_initiations", "friction_events")
    if line_messages:
        from . import line_telemetry as lt
        line_daily, coverage = _line_daily_aggregates(
            line_messages, group_contacts=group_contacts, contact=contact)
        for date_str, agg in line_daily.items():
            d = _date.fromisoformat(date_str)
            if d < first_d or d > last_d:
                continue
            i = idx_of(d)
            set_lane(i, "line_out_msgs", agg["out_msgs"])
            set_lane(i, "line_in_msgs", agg["in_msgs"])
            set_lane(i, "line_out_chars", agg["out_chars"])
            set_lane(i, "line_night_out", agg["night_out"])
            set_lane(i, "line_initiations", agg["initiations"])
            set_lane(i, "friction_events", agg["friction"])
            if agg["reply_latencies"]:
                set_lane(i, "line_reply_med_min", lt._median(agg["reply_latencies"]))

        if coverage:
            cov_start = max(_date.fromisoformat(coverage[0]), first_d)
            cov_end = min(_date.fromisoformat(coverage[1]), last_d)
            d = cov_start
            while d <= cov_end:
                date_str = d.isoformat()
                if date_str not in line_daily:   # 沈黙日 = 観測済み0
                    i = idx_of(d)
                    for lane_name in _SILENT_ZERO_LANES:
                        set_lane(i, lane_name, 0)
                d += timedelta(days=1)

    # lane 20-21 (github/leetcode importer 未実装) と 22-31 (reserved) は
    # 常に mask=0 — set_lane を一度も呼ばない (I-18: 欠測はマスクのみで表現)。

    mask_u32 = np.zeros(n_rows, dtype=np.uint32)
    for lane in range(KTEN_FEAT):
        mask_u32 |= (valid[:, lane].astype(np.uint32) << np.uint32(lane))

    rows = np.zeros(n_rows, dtype=ROW_DTYPE)
    rows["day"] = np.arange(n_rows, dtype=np.int32)
    rows["mask"] = mask_u32
    rows["f"] = np.where(valid, values, 0.0).astype(np.float32)   # 欠測値スロットは 0.0 (I-18)

    header = struct.pack(
        _HEADER_FMT, TENSOR_MAGIC, 1, n_rows, N_ACTIVE_FEATURES, ROW_SIZE,
        _to_epoch_day(first_d), FLAG_DYAD_SCOPE if scope == "dyad" else 0,
        content_hash64(daily))

    tmp = out_path.with_suffix(out_path.suffix + ".tmp")
    try:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        with open(tmp, "wb") as f:
            f.write(header)
            f.write(rows.tobytes())
        os.replace(tmp, out_path)
    except OSError as exc:
        raise TensorStoreError(
            f"{out_path}: 書き込み失敗 (mmap 保持中のハンドルが残っている可能性 — "
            f"release_tensor_mapping を先に呼んだか確認せよ): {exc}") from exc
    finally:
        tmp.unlink(missing_ok=True)
    return out_path
