import {
  dockItemClassName,
  isDockPrimarySelected,
  isMenuChromeSelected,
} from "../src/lib/mobileDockSelection";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("D-01 only matching id is selected when menu closed", () => {
  assertOk(isDockPrimarySelected("interview", "interview", false), "self");
  assertOk(!isDockPrimarySelected("interview", "record", false), "other");
  assertOk(!isDockPrimarySelected("interview", "interview", true), "menu open");
});

test("D-02 menu chrome when sheet open or menu-only surface", () => {
  assertOk(isMenuChromeSelected(true, true), "sheet open");
  assertOk(isMenuChromeSelected(false, false), "profile etc");
  assertOk(!isMenuChromeSelected(false, true), "dock surface");
});

test("D-03 class names never use bare active", () => {
  const on = dockItemClassName(true);
  const off = dockItemClassName(false);
  assertOk(on.includes("mobile-nav-item--active"), "active modifier");
  assertOk(off.includes("mobile-nav-item--idle"), "idle modifier");
  assertOk(!/\bactive\b/.test(on.replace("mobile-nav-item--active", "")), "no bare active");
  assertOk(!off.includes("active"), "idle has no active");
});

let failed = 0;
for (const { name, fn } of tests) {
  try {
    fn();
    console.log(`PASS ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`FAIL ${name}`, err);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
