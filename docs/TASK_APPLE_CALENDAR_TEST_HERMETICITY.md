# TASK: Apple カレンダーテストの非密封性 (隔離済み既知負債)

- 起票日: 2026-07-27
- 起票経緯: BLACKBOX SIMULATOR Phase 2 の品質ゲート実行中に観測。指揮官裁定により
  「シミュレータの進行には影響しない外部環境依存の既存負債」として別タスクへ隔離。
- 状態: **未着手**（本タスクは記録のみ。修正には別途の裁定が必要）
- 射程外の宣言: 本件は `blackbox_sim` と一切関係がない。Phase 2/3 の作業で触れてはならない。

## 症状

```
python -m pytest tests/test_apple_calendar_sync.py -q
FAILED tests/test_apple_calendar_sync.py::test_find_calendar_databases_custom_path
E   AssertionError: assert [PosixPath('/tmp/.../Calendar.sqlitedb'),
                            PosixPath('/Users/<user>/Library/Group Containers/
                                       group.com.apple.calendar/Calendar.sqlitedb')]
                        == [PosixPath('/tmp/.../Calendar.sqlitedb')]
    Left contains one more item
```

## 原因

`tests/test_apple_calendar_sync.py::test_find_calendar_databases_custom_path` は

```python
found = acs.find_calendar_databases([db])
assert found == [db.resolve()]
```

と、明示パスを 1 本渡した結果が**その 1 本だけ**であることを期待している。しかし
`find_calendar_databases` は明示パスに加えて既定の探索場所（`~/Library/Group Containers/
group.com.apple.calendar/` 等）も列挙する仕様であるため、**Apple カレンダーを実際に
使っている開発機では実 DB が必ず 1 件混ざる**。

つまりこれは実装のバグではなく、**テストが自分の実行環境から隔離されていない**
（非密封 / non-hermetic）という欠陥である。素な CI ランナーや Apple カレンダー未使用の
マシンでは PASS するため、環境によって結果が変わる。この「マシンによって割れるテスト」は、
本物の回帰を「またあの環境依存のやつだろう」で見逃させるので、放置コストが高い。

## 想定される修正方針（未裁定 — 実施前に裁定を仰げ）

1. **既定探索場所を注入可能にする**のが本筋。`find_calendar_databases(explicit, *,
   default_roots=DEFAULT_ROOTS)` として、テストは `default_roots=[]` を渡す。
   実装の公開シグネチャに触るため裁定が必要。
2. `monkeypatch` で既定 root を空にする。実装は無改造だが、テストが実装の内部名に
   依存するため、リネームで静かに空虚化する危険がある（Phase 1 の掟 1 と同型の罠）。
3. `assert set(found) >= {db.resolve()}` へ緩める。**非推奨** — 「明示パスだけを返す」
   という契約の検証を捨てることになり、テストが何も守らなくなる。

方針 1 を推奨する。1 と 2 のいずれでも、修正後は「Apple カレンダーのデータがある機と
ない機の両方で PASS すること」を実測で確認すること。

## 関連

- `.cursorrules` §品質ゲート「既知の失敗」に本件を明記済み。
- 同じく隔離されている既知負債: `test_integration.py::
  test_custom_theme_frontend_contract_static`（未着手 FE 機能の静的契約）。
