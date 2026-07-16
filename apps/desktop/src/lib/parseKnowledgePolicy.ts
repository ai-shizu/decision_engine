type JsonObject = Record<string, unknown>;

function fail(path: string, reason: string): never {
  throw new Error(`${path}: ${reason}`);
}

function asObject(value: unknown, path: string): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(path, "expected object");
  }
  return value as JsonObject;
}

function exactObject(
  value: unknown,
  required: readonly string[],
  optional: readonly string[],
  path: string,
): JsonObject {
  const object = asObject(value, path);
  const allowed = new Set([...required, ...optional]);
  for (const key of Object.keys(object)) {
    if (!allowed.has(key)) fail(path, "unexpected field");
  }
  for (const key of required) {
    if (!Object.prototype.hasOwnProperty.call(object, key)) fail(path, "missing field");
  }
  return object;
}

export interface KnowledgePolicyState {
  schema: "knowledge_policy.v1";
  enabled: boolean;
}

export function parseKnowledgePolicy(value: unknown): KnowledgePolicyState {
  const object = exactObject(
    value,
    ["schema", "enabled"],
    [],
    "knowledge_policy",
  );
  if (object.schema !== "knowledge_policy.v1") {
    fail("knowledge_policy.schema", "expected knowledge_policy.v1");
  }
  if (typeof object.enabled !== "boolean") {
    fail("knowledge_policy.enabled", "expected boolean");
  }
  for (const banned of ["query", "message", "error", "url", "research_id"]) {
    if (Object.prototype.hasOwnProperty.call(object, banned)) {
      fail(`knowledge_policy.${banned}`, "forbidden field");
    }
  }
  return {
    schema: "knowledge_policy.v1",
    enabled: object.enabled,
  };
}
