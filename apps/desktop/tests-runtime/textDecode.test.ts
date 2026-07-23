import { readTextLenient } from "../src/lib/textDecode";

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function fileFromBytes(bytes: number[], name = "line.txt"): File {
  return new File([new Uint8Array(bytes)], name, { type: "text/plain" });
}

const UTF8_BOM = [0xef, 0xbb, 0xbf];
const UTF16LE_BOM = [0xff, 0xfe];
const UTF16BE_BOM = [0xfe, 0xff];

function utf16leBytes(text: string): number[] {
  const out: number[] = [];
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    out.push(code & 0xff, (code >> 8) & 0xff);
  }
  return out;
}

function utf16beBytes(text: string): number[] {
  const out: number[] = [];
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    out.push((code >> 8) & 0xff, code & 0xff);
  }
  return out;
}

async function main(): Promise<void> {
  // T-01: plain UTF-8, no BOM.
  {
    const text = "13:45\tAlice\tこんにちは";
    const decoded = await readTextLenient(fileFromBytes([...new TextEncoder().encode(text)]));
    assertOk(decoded === text, "T-01 utf-8 without bom decodes exactly");
  }

  // T-02: UTF-8 with BOM — TextDecoder("utf-8") strips it per WHATWG spec.
  {
    const text = "2024/01/15(月)\n13:45\tAlice\tこんにちは";
    const bytes = [...UTF8_BOM, ...new TextEncoder().encode(text)];
    const decoded = await readTextLenient(fileFromBytes(bytes));
    assertOk(!decoded.startsWith("\uFEFF"), "T-02 no leading BOM char");
    assertOk(decoded === text, "T-02 utf-8 with bom decodes to exact body");
  }

  // T-03: UTF-16 LE with BOM must not fall through to Shift-JIS (which would
  // mojibake every 2-byte UTF-16 unit as 1-2 unrelated Shift-JIS characters).
  {
    const text = "2024/01/15(月)\n13:45\tAlice\tこんにちは";
    const bytes = [...UTF16LE_BOM, ...utf16leBytes(text)];
    const decoded = await readTextLenient(fileFromBytes(bytes));
    assertOk(decoded === text, "T-03 utf-16le with bom decodes exactly");
  }

  // T-04: UTF-16 BE with BOM.
  {
    const text = "2024/01/15(月)\n13:45\tAlice\tこんにちは";
    const bytes = [...UTF16BE_BOM, ...utf16beBytes(text)];
    const decoded = await readTextLenient(fileFromBytes(bytes));
    assertOk(decoded === text, "T-04 utf-16be with bom decodes exactly");
  }

  // T-05: invalid UTF-8 byte sequence falls back to Shift-JIS decode rather
  // than throwing or mojibaking silently (mirrors core/es_manager.py policy).
  {
    // 0x82 0xa0 is Shift-JIS for "あ"; invalid as a standalone UTF-8 sequence.
    const decoded = await readTextLenient(fileFromBytes([0x82, 0xa0]));
    assertOk(decoded === "あ", "T-05 shift-jis fallback decodes correctly");
  }

  // T-06: empty file never throws and yields empty string.
  {
    const decoded = await readTextLenient(fileFromBytes([]));
    assertOk(decoded === "", "T-06 empty file decodes to empty string");
  }

  console.log("PASS textDecode readTextLenient BOM/UTF-16/Shift-JIS robustness");
}

void main();
