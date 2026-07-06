# -*- coding: utf-8 -*-
"""search_daemon (Target Bravo: mmap ゼロコピー IPC) の決定論的テスト。

2 層構成:
  1. fake プロセス (stdin/stdout モック) によるプロトコル検証 — 外部依存なし・決定論。
  2. ビルド済み exe がある環境でのみ実行するゲート付き E2E (不在なら SKIP、FAIL しない)。

実データには一切触れない (scratch / index は全て一時ディレクトリ)。
"""
import json
import queue
import struct
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.paths import SEARCH_EXE  # noqa: E402
from core.search_daemon import (  # noqa: E402
    DIM,
    HEADER_SIZE,
    QUERY_OFFSET,
    QUERY_SIZE,
    RESULT_SIZE,
    RESULTS_OFFSET,
    SCRATCH_MAGIC,
    SCRATCH_MAX_K,
    SCRATCH_SIZE,
    SearchDaemonClient,
    SearchDaemonError,
)


# ---------------------------------------------------------------- レイアウト相互検証
def test_layout_cross_validation() -> None:
    """C++ 側 static_assert が釘付けにしている数値と Python struct の一致。
    どちらかを変えたら両方を同時に変えること (search_engine.cpp ScratchBuffer)。"""
    assert HEADER_SIZE == 24
    assert QUERY_OFFSET == 24
    assert QUERY_SIZE == DIM * 4 == 1536
    assert RESULTS_OFFSET == 1560
    assert RESULT_SIZE == 8
    assert SCRATCH_MAX_K == 64
    assert SCRATCH_SIZE == 2072
    # 自然整列の検証: u64 (seq) が 8 の倍数、results 先頭も 8 の倍数
    assert struct.calcsize("<8s") % 8 == 0
    assert RESULTS_OFFSET % 8 == 0
    print("  layout cross validation OK")


# ---------------------------------------------------------------- fake プロセス
class _QueueReader:
    """proc.stdout の代役。put(None) が EOF。"""

    def __init__(self, q: queue.Queue):
        self._q = q

    def __iter__(self):
        return self

    def __next__(self) -> str:
        line = self._q.get()
        if line is None:
            raise StopIteration
        return line


class FakeDaemonProc:
    """search_engine --daemon の Python 実装 (プロトコル検証用)。

    scratch ファイルを実際に読み書きするため、データプレーンの
    バイトレイアウトも同時に検証される。search への応答は決定論:
    chunk_id = 100+i, score = 1.0 - 0.1*i を min(top_k, 3) 件返す。
    """

    def __init__(self, args: list[str]):
        assert args[1] == "--daemon", args
        self.scratch_path = Path(args[2])
        self._q: queue.Queue = queue.Queue()
        self.stdout = _QueueReader(self._q)
        self.stdin = self
        self.returncode = None
        self._buf = ""
        self.requests: list[dict] = []
        scratch = self.scratch_path.read_bytes()
        assert scratch[:8] == SCRATCH_MAGIC, "scratch magic 未初期化"
        assert len(scratch) == SCRATCH_SIZE
        self._q.put(json.dumps({"event": "ready", "max_k": SCRATCH_MAX_K,
                                "scratch_bytes": SCRATCH_SIZE}) + "\n")

    # ---- stdin 側 (client が write する)
    def write(self, s: str) -> None:
        self._buf += s
        while "\n" in self._buf:
            line, self._buf = self._buf.split("\n", 1)
            if line.strip():
                self._handle(json.loads(line))

    def flush(self) -> None:
        pass

    def close(self) -> None:
        if self.returncode is None:
            self.returncode = 0
        self._q.put(None)

    # ---- プロセス制御
    def poll(self):
        return self.returncode

    def wait(self, timeout=None):
        self.returncode = 0
        return 0

    def terminate(self):
        self.returncode = 1

    def kill(self):
        self.returncode = 1

    # ---- コマンド処理 (C++ 側と同じ検証をする)
    def _handle(self, req: dict) -> None:
        self.requests.append(req)
        cmd = req.get("cmd")
        if cmd == "shutdown":
            self._q.put('{"ok":true}\n')
            self.returncode = 0
            self._q.put(None)
            return
        if cmd == "ping":
            self._q.put('{"ok":true}\n')
            return
        if cmd == "remap":
            self._q.put('{"ok":true}\n')
            return
        if cmd != "search":
            self._q.put(json.dumps({"ok": False, "error": f"unknown cmd: {cmd}"}) + "\n")
            return
        if "missing" in req.get("index", ""):
            self._q.put(json.dumps(
                {"ok": False, "seq": req["seq"],
                 "error": "cannot map index: " + req["index"]}) + "\n")
            return
        # client が scratch を mmap 中のため truncate 禁止 — "r+b" で in-place 更新
        with open(self.scratch_path, "r+b") as f:
            raw = bytearray(f.read())
            magic, seq, top_k, _ = struct.unpack_from("<8sQII", raw, 0)
            assert magic == SCRATCH_MAGIC
            if seq != req["seq"]:
                self._q.put(json.dumps(
                    {"ok": False, "seq": req["seq"],
                     "error": f"seq mismatch (scratch={seq}, request={req['seq']})"}) + "\n")
                return
            count = min(top_k, 3)
            for i in range(SCRATCH_MAX_K):
                cid, score = (100 + i, 1.0 - 0.1 * i) if i < count else (-1, 0.0)
                struct.pack_into("<if", raw, RESULTS_OFFSET + i * RESULT_SIZE, cid, score)
            struct.pack_into("<I", raw, 20, count)
            f.seek(0)
            f.write(raw)
        self._q.put(json.dumps({"ok": True, "seq": seq, "count": count,
                                "elapsed_us": 1.0}) + "\n")


def _fake_client(tmp: Path) -> tuple[SearchDaemonClient, list]:
    holder: list = []

    def spawn(args):
        proc = FakeDaemonProc(args)
        holder.append(proc)
        return proc

    cli = SearchDaemonClient(exe=Path(sys.executable),  # 存在チェックを通すだけ
                             scratch_path=tmp / "scratch.bin", spawn=spawn)
    cli.start()
    return cli, holder


def test_fake_protocol_search() -> None:
    """データプレーン往復: query 書き込み → 応答 → results 読み出し。"""
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        cli, holder = _fake_client(tmp)
        try:
            qbytes = struct.pack(f"<{DIM}f", *([0.5] * DIM))
            hits = cli.search(tmp / "vectors.bin", qbytes, top_k=5)
            got = [(c, round(s, 4)) for c, s in hits]   # f32 往復の丸め
            assert got == [(100, 1.0), (101, 0.9), (102, 0.8)], got
            # scratch に書かれた query / top_k / seq を fake 側から検証
            raw = cli.scratch_path.read_bytes()
            _, seq, top_k, count = struct.unpack_from("<8sQII", raw, 0)
            assert seq == 1 and top_k == 5 and count == 3
            assert raw[QUERY_OFFSET:QUERY_OFFSET + QUERY_SIZE] == qbytes
            # 2 回目で seq が単調増加する
            cli.search(tmp / "vectors.bin", qbytes, top_k=2)
            assert holder[0].requests[-1]["seq"] == 2
            # 未使用スロット (chunk_id=-1) が結果に混入しない
            hits2 = cli.search(tmp / "vectors.bin", qbytes, top_k=SCRATCH_MAX_K)
            assert len(hits2) == 3
        finally:
            cli.close()
        assert holder[0].returncode == 0, "shutdown でクリーンに終了していない"
        assert not cli.scratch_path.exists(), "scratch が掃除されていない"
    print("  fake protocol search OK")


def test_fake_protocol_errors() -> None:
    """エラー応答は SearchDaemonError に変換され、握り潰されない。"""
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        cli, _ = _fake_client(tmp)
        try:
            qbytes = struct.pack(f"<{DIM}f", *([0.0] * DIM))
            # index 不在 → ok:false → 例外 (Step 2 のフォールバック連鎖の入口)
            try:
                cli.search(tmp / "missing.bin", qbytes, top_k=3)
                raise AssertionError("SearchDaemonError が出ていない")
            except SearchDaemonError as e:
                assert "cannot map index" in str(e)
            # クエリ長不正は送信前に検出
            try:
                cli.search(tmp / "vectors.bin", b"\x00" * 100, top_k=3)
                raise AssertionError("クエリ長検証が働いていない")
            except SearchDaemonError as e:
                assert "1536" in str(e)
            assert cli.ping()
            cli.remap(tmp / "vectors.bin")
        finally:
            cli.close()
    print("  fake protocol errors OK")


def test_missing_exe_raises() -> None:
    with tempfile.TemporaryDirectory() as d:
        cli = SearchDaemonClient(exe=Path(d) / "no_such.exe",
                                 scratch_path=Path(d) / "s.bin")
        try:
            cli.start()
            raise AssertionError("exe 不在で start が成功してしまった")
        except SearchDaemonError:
            pass
        finally:
            cli.close()
    print("  missing exe raises OK")


# ---------------------------------------------------------------- エンジン統合 (Step 2)
class StubDaemon:
    """ConsultationEngine のフォールバック連鎖検証用スタブ。"""

    def __init__(self, hits=None):
        self.hits = hits if hits is not None else [(2, 0.5)]
        self.calls: list = []
        self.closed = False

    def search(self, bin_path, qvec, top_k):
        self.calls.append(("search", top_k))
        return self.hits

    def remap(self, bin_path):
        self.calls.append(("remap", Path(bin_path).name))

    def close(self):
        self.closed = True


def _engine_fixture(tmp: Path):
    """一時 index + metadata + クエリ (numpy)。実データ非接触。"""
    import numpy as np
    import core.consultation_engine as ce
    _write_test_index(tmp / "vectors.bin", [_one_hot(i) for i in range(4)])
    meta = {"chunks": [{"id": i, "date": f"2026-07-0{i + 1}", "text": f"chunk{i}"}
                       for i in range(4)]}
    (tmp / "metadata.json").write_text(
        json.dumps(meta, ensure_ascii=False), encoding="utf-8")
    qvec = np.zeros(DIM, dtype=np.float32)
    qvec[2] = 1.0
    return ce, qvec


def test_engine_daemon_primary() -> None:
    """Primary: デーモンが生きていれば daemon.search の結果が使われる。"""
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        ce, qvec = _engine_fixture(tmp)
        eng = ce.ConsultationEngine()
        stub = StubDaemon(hits=[(2, 0.5), (0, 0.1)])
        eng._search_daemon = stub
        hits = eng.search_index(tmp / "vectors.bin", tmp / "metadata.json",
                                qvec, top_k=2)
        assert [h["id"] for h in hits] == [2, 0], hits
        assert hits[0]["date"] == "2026-07-03"   # metadata 突合は Python 側の責務のまま
        assert stub.calls == [("search", 2)], stub.calls
    print("  engine daemon primary OK")


def test_engine_fallback_to_numpy() -> None:
    """デーモン死亡 → drop (respawn しない) → 1-shot exe 不在 → NumPy が正解を返す。"""
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        ce, qvec = _engine_fixture(tmp)

        class DeadDaemon(StubDaemon):
            def search(self, *a, **k):
                raise SearchDaemonError("dead")

        eng = ce.ConsultationEngine()
        dead = DeadDaemon()
        eng._search_daemon = dead
        orig_exe = ce.SEARCH_EXE
        ce.SEARCH_EXE = tmp / "no_such.exe"   # Secondary も不在にして Tertiary まで落とす
        try:
            hits = eng.search_index(tmp / "vectors.bin", tmp / "metadata.json",
                                    qvec, top_k=2)
        finally:
            ce.SEARCH_EXE = orig_exe
        assert dead.closed, "死んだデーモンが close されていない"
        assert eng._search_daemon is None and eng._search_daemon_failed
        assert hits and hits[0]["id"] == 2, hits   # NumPy 経路が正しい Top を返す
        assert eng._get_search_daemon() is None, "障害後に respawn を試みている"
    print("  engine fallback to numpy OK")


def test_engine_release_before_rebuild() -> None:
    """再構築前のロック解放: remap が送られる / remap 失敗時はデーモンごと終了。"""
    import core.consultation_engine as ce
    eng = ce.ConsultationEngine()
    stub = StubDaemon()
    eng._search_daemon = stub
    eng._release_index_mapping(Path("vectors.bin"))
    assert ("remap", "vectors.bin") in stub.calls

    class FailingRemap(StubDaemon):
        def remap(self, bin_path):
            raise SearchDaemonError("broken pipe")

    eng2 = ce.ConsultationEngine()
    failing = FailingRemap()
    eng2._search_daemon = failing
    eng2._release_index_mapping(Path("vectors.bin"))
    assert failing.closed, "remap 失敗時にデーモンが close されていない (mmap 解放が保証されない)"
    assert eng2._search_daemon is None

    # デーモン未起動なら何もしない (例外を出さない)
    eng3 = ce.ConsultationEngine()
    eng3._release_index_mapping(Path("vectors.bin"))
    print("  engine release before rebuild OK")


def test_engine_shutdown_closes_daemon() -> None:
    """shutdown はデーモンを閉じるが failed フラグは立てない (再利用時に再起動可)。"""
    import core.consultation_engine as ce
    eng = ce.ConsultationEngine()
    stub = StubDaemon()
    eng._search_daemon = stub
    eng.shutdown()
    assert stub.closed and eng._search_daemon is None
    assert not eng._search_daemon_failed
    print("  engine shutdown closes daemon OK")


# ---------------------------------------------------------------- ゲート付き E2E
def _write_test_index(path: Path, vectors: list[list[float]]) -> None:
    """pipeline.py と同一の PKBVEC01 / AoSoA レイアウトを stdlib だけで書く。"""
    n = len(vectors)
    blocks = (n + 3) // 4
    with open(path, "wb") as f:
        f.write(struct.pack("<8sIIIIII", b"PKBVEC01", DIM, 4, n, blocks,
                            DIM * 4 * 4 + 4 * 4, 0))
        for b in range(blocks):
            lanes = [vectors[b * 4 + l] if b * 4 + l < n else [0.0] * DIM
                     for l in range(4)]
            for dim in range(DIM):
                f.write(struct.pack("<4f", *(lanes[l][dim] for l in range(4))))
            f.write(struct.pack(
                "<4i", *((b * 4 + l) if b * 4 + l < n else -1 for l in range(4))))


def _one_hot(i: int) -> list[float]:
    v = [0.0] * DIM
    v[i] = 1.0
    return v


def test_e2e_real_daemon() -> None:
    """実 exe とのゼロコピー往復。one-hot ベクトルで内積が既知の値になる構成。"""
    if not SEARCH_EXE.exists():
        print(f"  E2E SKIP (exe 不在: {SEARCH_EXE})")
        return
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        # vec_i = e_i → dot(query, vec_i) = query[i]。6 本 = 2 ブロック (パディング 2 レーン)
        _write_test_index(tmp / "vectors.bin", [_one_hot(i) for i in range(6)])
        weights = [0.9, 0.5, 0.7, 0.1, 0.3, 0.8]
        q = weights + [0.0] * (DIM - len(weights))

        cli = SearchDaemonClient(exe=SEARCH_EXE, scratch_path=tmp / "scratch.bin")
        try:
            cli.start()
            hits = cli.search(tmp / "vectors.bin", struct.pack(f"<{DIM}f", *q), 3)
            got = [(cid, round(s, 4)) for cid, s in hits]
            assert got == [(0, 0.9), (5, 0.8), (2, 0.7)], got

            # 2 クエリ目 (index は mmap キャッシュ再利用): 重み反転で順位が変わる
            q2 = [0.1, 0.9, 0.2, 0.8, 0.0, 0.0] + [0.0] * (DIM - 6)
            hits2 = cli.search(tmp / "vectors.bin", struct.pack(f"<{DIM}f", *q2), 2)
            assert [(c, round(s, 4)) for c, s in hits2] == [(1, 0.9), (3, 0.8)], hits2

            # top_k > 実ベクトル数 → 全件 (パディングレーン -1 は返らない)
            hits3 = cli.search(tmp / "vectors.bin", struct.pack(f"<{DIM}f", *q), 50)
            assert len(hits3) == 6, hits3

            # seq 不一致は C++ 側が検出して拒否する (黙って古いクエリを検索しない)
            cli._seq += 1
            struct.pack_into("<Q", cli._mm, 8, cli._seq + 999)
            resp = cli._request({"cmd": "search", "seq": cli._seq,
                                 "index": str(tmp / "vectors.bin")})
            assert resp["ok"] is False and "seq mismatch" in resp["error"], resp

            # remap → 再検索が正常動作 (遅延リマップ)
            cli.remap(tmp / "vectors.bin")
            hits4 = cli.search(tmp / "vectors.bin", struct.pack(f"<{DIM}f", *q), 1)
            assert [(c, round(s, 4)) for c, s in hits4] == [(0, 0.9)], hits4

            assert cli.ping()

            # 存在しない index はエラー応答 (デーモンは死なない)
            try:
                cli.search(tmp / "no_such.bin", struct.pack(f"<{DIM}f", *q), 3)
                raise AssertionError("index 不在でエラーになっていない")
            except SearchDaemonError:
                pass
            assert cli.ping(), "エラー後にデーモンが死んでいる"
        finally:
            cli.close()
    print("  E2E real daemon OK")


def test_e2e_eof_suicide() -> None:
    """親の死 (stdin パイプ切断) でデーモンが自己終了する — ゾンビ化防止の要。"""
    if not SEARCH_EXE.exists():
        print(f"  E2E SKIP (exe 不在: {SEARCH_EXE})")
        return
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        cli = SearchDaemonClient(exe=SEARCH_EXE, scratch_path=tmp / "scratch.bin")
        try:
            cli.start()
            proc = cli.proc
            proc.stdin.close()          # 親の死をシミュレート (shutdown は送らない)
            deadline = time.time() + 5.0
            while proc.poll() is None and time.time() < deadline:
                time.sleep(0.05)
            assert proc.poll() == 0, f"EOF 後も生存 (returncode={proc.poll()})"
        finally:
            cli.close()
    print("  E2E EOF suicide OK")


if __name__ == "__main__":
    test_layout_cross_validation()
    test_fake_protocol_search()
    test_fake_protocol_errors()
    test_missing_exe_raises()
    test_engine_daemon_primary()
    test_engine_fallback_to_numpy()
    test_engine_release_before_rebuild()
    test_engine_shutdown_closes_daemon()
    test_e2e_real_daemon()
    test_e2e_eof_suicide()
    print("test_search_daemon: ALL PASS")
