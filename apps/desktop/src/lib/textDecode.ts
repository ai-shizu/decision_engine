/**
 * W-33 (SPEC_FOXTROT_UI.md §6): File.text() は常に UTF-8 でデコードする。
 * メモ帳の ANSI 保存 (cp932) の .txt を読むと本文が文字化けしたまま聖域に
 * 書き込まれる — バックエンドの core/es_manager.py::_read_text_lenient と
 * 同じ配慮をフロントにも入れる。ブラウザ標準 API のみ (決定論的)。
 */
export async function readTextLenient(file: File): Promise<string> {
  const buf = await file.arrayBuffer();
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(buf);
  } catch {
    return new TextDecoder("shift_jis").decode(buf);
  }
}
