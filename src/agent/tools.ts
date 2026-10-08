import { z } from "zod";
import type { ObjScriptInterpreter } from "../interpreter";
import { ScriptSchema, type Script } from "../schema";

/** Wherever scripts live: database, files, KV... */
export interface ScriptStore {
  get(id: string): Promise<Script | undefined>;
  list(): Promise<{ id: string; kind: "command" | "module"; description?: string }[]>;
  put(id: string, kind: "command" | "module", script: Script): Promise<void>;
  delete(id: string): Promise<boolean>;
}

export interface Tool<I extends z.ZodType = z.ZodType> {
  name: string;
  description: string;
  input: I;
  execute: (input: z.infer<I>) => Promise<unknown>;
}
const tool = <I extends z.ZodType>(t: Tool<I>) => t;

const idOf = (s: Script) => s.command?.name ?? s.name;
const kindOf = (s: Script) => s.name ? "module" : "command";

export function objscriptTools<C>(interp: ObjScriptInterpreter<C>, store: ScriptStore): Tool[] {
  return [
    tool({
      name: "objscript_catalog",
      description: "List the built-in functions, host capabilities and library modules available to scripts. Call this before writing a script.",
      input: z.object({}),
      execute: async () => interp.catalog(),
    }),
    tool({
      name: "objscript_check",
      description: "Type-check a script without running it. Returns every error with a JSON path, code and hint.",
      input: z.object({ script: ScriptSchema }),
      execute: async ({ script }) => interp.check(script),
    }),
    tool({
      name: "objscript_run",
      description:
        'Run a script once with inputs. `mocks` fakes capabilities by path, e.g. {"host/http/fetch": {"status": 200, "body": {...}}}. Unmocked capabilities make real calls.',
      input: z.object({
        script: ScriptSchema,
        inputs: z.record(z.string(), z.unknown()).default({}),
        mocks: z.record(z.string(), z.unknown()).optional(),
      }),
      execute: async ({ script, inputs, mocks }) => interp.run(script, inputs, { mocks }),
    }),
    tool({
      name: "objscript_save",
      description:
        "Save a script. It must pass objscript_check first. Commands are saved under command.name; library modules under name, and become importable immediately.",
      input: z.object({ script: ScriptSchema }),
      execute: async ({ script }) => {
        const check = interp.check(script);
        if (!check.ok) return check;
        const id = idOf(script);
        if (!id) return { ok: false, error: "script needs command.name (commands) or name (modules)" };
        await store.put(id, kindOf(script), script);
        if (kindOf(script) === "module") interp.addModule(script);
        return { ok: true, id };
      },
    }),
    tool({
      name: "objscript_get",
      description: "Fetch a saved script by id (command name or module name).",
      input: z.object({ id: z.string() }),
      execute: async ({ id }) => (await store.get(id)) ?? { ok: false, error: `no script ${id}` },
    }),
    tool({
      name: "objscript_list",
      description: "List saved scripts.",
      input: z.object({}),
      execute: async () => store.list(),
    }),
    tool({
      name: "objscript_delete",
      description: "Delete a saved script. Deleting a module breaks any script that imports it.",
      input: z.object({ id: z.string() }),
      execute: async ({ id }) => {
        const existing = await store.get(id);
        const deleted = await store.delete(id);
        if (deleted && existing && kindOf(existing) === "module") interp.removeModule(id);
        return { ok: deleted };
      },
    }),
  ];
}

/** Provider-agnostic definitions: { name, description, input_schema }. */
export function toolDefinitions(tools: Tool[]) {
  return tools.map((t) => ({ name: t.name, description: t.description, input_schema: z.toJSONSchema(t.input) }));
}

/**
 * Run a tool call from the model. If Zod rejects a script, fall back to the
 * Rust checker's diagnostics, which are more precise and come with hints.
 */
export async function callTool<C>(
  tools: Tool[],
  interp: ObjScriptInterpreter<C>,
  name: string,
  rawInput: unknown,
): Promise<unknown> {
  const t = tools.find((t) => t.name === name);
  if (!t) return { ok: false, error: `unknown tool ${name}` };

  const parsed = t.input.safeParse(rawInput);
  if (parsed.success) return t.execute(parsed.data);

  const script = (rawInput as { script?: unknown } | null)?.script;
  if (script && typeof script === "object") {
    const check = interp.check(script as Script);
    if (!check.ok) return check;
  }
  return {
    ok: false,
    errors: parsed.error.issues.map((i) => ({
      path: "/" + i.path.map(String).join("/"),
      code: "invalid_shape",
      message: i.message,
    })),
  };
}