/**
 * W-33 (SPEC_FOXTROT_UI.md §6): File.text() は常に UTF-8 でデコードする。
 * メモ帳の ANSI 保存 (cp932) の .txt を読むと本文が文字化けしたまま聖域に
 * 書き込まれる — バックエンドの core/es_manager.py::_read_text_lenient と
 * 同じ配慮をフロントにも入れる。ブラウザ標準 API のみ (決定論的)。
 *
 * M20 データ連携 Part 1: LINE エクスポートは端末により UTF-16 (BOM 付き) の
 * 場合がある。BOM は明示的で曖昧さがないため、他のどの推測より先に判定する
 * (BOM 無視だと UTF-16 の生バイト列が UTF-8/Shift-JIS どちらの経路でも
 * 完全な文字化けになる)。
 */
export async function readTextLenient(file: File): Promise<string> {
  const buf = await file.arrayBuffer();
  const bytes = new Uint8Array(buf);

  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) {
    return new TextDecoder("utf-16le").decode(buf);
  }
  if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) {
    return new TextDecoder("utf-16be").decode(buf);
  }

  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(buf);
  } catch {
    text = new TextDecoder("shift_jis").decode(buf);
  }
  // Defensive: strip a leading BOM that survived decoding (e.g. a stray UTF-8
  // BOM prefix on an otherwise non-UTF-8 file, decoded via the Shift-JIS
  // fallback above, which does not know to strip it).
  return text.replace(/^\uFEFF/, "");
}
