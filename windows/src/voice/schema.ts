// The live assistant's tool declarations (tools.json, also sent to the provider by
// the native gateway) and a validator for the JSON-schema subset they use. Tool
// arguments come from the model: they are untrusted until this accepts them.

import TOOLS from "./tools.json";

export interface JsonSchema {
  type?: "object" | "string" | "number" | "integer" | "boolean";
  properties?: Record<string, JsonSchema>;
  required?: string[];
  additionalProperties?: boolean;
  enum?: readonly unknown[];
  minimum?: number;
  maximum?: number;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
}
export interface ToolSpec { name: string; mutating: boolean; description: string; parameters: JsonSchema }

export const TOOL_SPECS: readonly ToolSpec[] = TOOLS as ToolSpec[];
const BY_NAME = new Map(TOOL_SPECS.map(spec => [spec.name, spec]));
export const toolSpec = (name: string): ToolSpec | undefined => BY_NAME.get(name);

/** Null when `value` satisfies `schema`, otherwise a short reason naming the path. */
export function validate(value: unknown, schema: JsonSchema, path = "args"): string | null {
  switch (schema.type) {
    case "object": {
      if (!value || typeof value !== "object" || Array.isArray(value)) return `${path} must be an object`;
      const record = value as Record<string, unknown>;
      const props = schema.properties ?? {};
      for (const key of schema.required ?? []) if (!(key in record)) return `${path}.${key} is required`;
      for (const [key, item] of Object.entries(record)) {
        const sub = Object.prototype.hasOwnProperty.call(props, key) ? props[key] : undefined;
        if (!sub) { if (schema.additionalProperties === false) return `${path}.${key} is not allowed`; continue; }
        const reason = validate(item, sub, `${path}.${key}`);
        if (reason) return reason;
      }
      return null;
    }
    case "string":
      if (typeof value !== "string") return `${path} must be a string`;
      if (schema.minLength !== undefined && [...value].length < schema.minLength) return `${path} is too short`;
      if (schema.maxLength !== undefined && [...value].length > schema.maxLength) return `${path} is too long`;
      if (schema.pattern !== undefined && !new RegExp(schema.pattern, "u").test(value)) return `${path} has the wrong format`;
      break;
    case "number": case "integer":
      if (typeof value !== "number" || !Number.isFinite(value)) return `${path} must be a number`;
      if (schema.type === "integer" && !Number.isInteger(value)) return `${path} must be an integer`;
      if (schema.minimum !== undefined && value < schema.minimum) return `${path} must be at least ${schema.minimum}`;
      if (schema.maximum !== undefined && value > schema.maximum) return `${path} must be at most ${schema.maximum}`;
      break;
    case "boolean":
      if (typeof value !== "boolean") return `${path} must be true or false`;
      break;
  }
  if (schema.enum && !schema.enum.includes(value)) return `${path} must be one of ${schema.enum.join(", ")}`;
  return null;
}

export const MAX_ARGUMENTS = 32_000;

export const MAX_APP_ARGUMENTS = 16 * 1024;

/** App (registry) tools: their schemas live natively; here only a JSON object of at most 16 KB. */
export function parseAppToolArgs(raw: unknown): { ok: true; args: Record<string, unknown> } | { ok: false; reason: string } {
  if (typeof raw !== "string" || new TextEncoder().encode(raw).length > MAX_APP_ARGUMENTS) return { ok: false, reason: "arguments must be a JSON string of at most 16 KB" };
  let args: unknown;
  try { args = raw.trim() ? JSON.parse(raw) : {}; } catch { return { ok: false, reason: "arguments are not valid JSON" }; }
  if (!args || typeof args !== "object" || Array.isArray(args)) return { ok: false, reason: "arguments must be an object" };
  return { ok: true, args: args as Record<string, unknown> };
}

/** Parses and validates a function call's raw `arguments` string. */
export function parseToolArgs(name: string, raw: unknown): { ok: true; args: Record<string, unknown> } | { ok: false; reason: string } {
  const spec = toolSpec(name);
  if (!spec) return { ok: false, reason: `unknown tool ${name.slice(0, 64)}` };
  if (typeof raw !== "string" || raw.length > MAX_ARGUMENTS) return { ok: false, reason: "arguments must be a JSON string" };
  let args: unknown;
  try { args = raw.trim() ? JSON.parse(raw) : {}; } catch { return { ok: false, reason: "arguments are not valid JSON" }; }
  const reason = validate(args, spec.parameters);
  return reason ? { ok: false, reason } : { ok: true, args: args as Record<string, unknown> };
}
