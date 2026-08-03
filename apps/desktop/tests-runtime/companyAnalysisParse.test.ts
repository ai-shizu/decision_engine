/**
 * Company analysis markdown parser (external deps: zero).
 */
import { parseCompanyAnalysis } from "../src/lib/companyAnalysisParse";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function assertEqual(actual: unknown, expected: unknown, msg: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${msg}: expected ${e} got ${a}`);
}

test("CA-01 four sections normal", () => {
  const raw = `## 面接で深掘りされやすい事業リスク
- リスクA
- リスクB
## 直近の企業戦略の変化
- 戦略1
## 予想される質問
- 質問1
## 逆質問の候補
- 逆質問1
`;
  const sections = parseCompanyAnalysis(raw);
  assertEqual(sections.length, 4, "count");
  assertEqual(sections[0].heading, "面接で深掘りされやすい事業リスク", "h1");
  assertEqual(sections[0].items, ["リスクA", "リスクB"], "items");
  assertEqual(sections[2].items, ["質問1"], "q");
});

test("CA-02 no headings → empty", () => {
  assertEqual(parseCompanyAnalysis("ただの本文\n- 箇条書き"), [], "empty");
});

test("CA-03 non-bullet lines ignored", () => {
  const sections = parseCompanyAnalysis(`## 見出し
説明文は無視
- 採用
もっと無視
- これも
`);
  assertEqual(sections.length, 1, "one");
  assertEqual(sections[0].items, ["採用", "これも"], "bullets only");
});

test("CA-04 partial sections not invented", () => {
  const sections = parseCompanyAnalysis(`## 予想される質問
- だけある
`);
  assertEqual(sections.length, 1, "only present");
  assertEqual(sections[0].heading, "予想される質問", "heading");
  assertOk(
    !sections.some((s) => s.heading.includes("逆質問")),
    "no invented reverse-Q section",
  );
});

test("CA-05 empty input", () => {
  assertEqual(parseCompanyAnalysis(""), [], "empty");
  assertEqual(parseCompanyAnalysis("   \n  "), [], "ws");
});

let failed = 0;
for (const t of tests) {
  try {
    t.fn();
    console.log(`PASS ${t.name}`);
  } catch (e) {
    failed += 1;
    console.log(`FAIL ${t.name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) throw new Error(`${failed} tests failed`);
