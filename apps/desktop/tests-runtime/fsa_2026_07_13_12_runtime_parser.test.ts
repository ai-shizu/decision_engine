import type * as EngineModule from "../src/lib/engine";

declare function require(id: string): unknown;

type TauriCoreModule = {
  invoke: (command: string, args?: unknown) => Promise<unknown>;
};

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

async function main(): Promise<void> {
  const tauriCore = require("@tauri-apps/api/core") as TauriCoreModule;
  const originalInvoke = tauriCore.invoke;
  tauriCore.invoke = async () => ({ status: 17, offline: true });

  let rejected = false;
  try {
    const engine = require("../src/lib/engine") as typeof EngineModule;
    await engine.engineHealth();
  } catch {
    rejected = true;
  } finally {
    tauriCore.invoke = originalInvoke;
  }

  assertOk(
    rejected,
    "malformed invoke payload crossed the runtime parser boundary",
  );
  console.log("PASS FSA-2026-07-13-12 malformed invoke payload is rejected");
}

void main().catch((error: unknown) => {
  console.log(`FAIL FSA-2026-07-13-12: ${error instanceof Error ? error.message : String(error)}`);
  throw error;
});
