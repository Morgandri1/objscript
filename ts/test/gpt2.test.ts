import { describe, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { ObjScriptInterpreter } from "../src";

// Point GPT2_DIR at a folder made by examples/gpt2/generate.py (a --tiny one runs in seconds).
// GPT2_EXPECT is the reference's new_ids, e.g. "[43895,5805,8086,9923]".
const dir = process.env.GPT2_DIR;
const run = dir && existsSync(`${dir}/gpt2.json`) ? test : test.skip;

describe("GPT-2 in ObjScript, through the WASM interpreter", () => {
  run("generates the same tokens as the NumPy reference", async () => {
    const interp = new ObjScriptInterpreter({ limits: { fuel: null, max_size: null, max_host_calls: null } });
    await interp.loadModuleDir(`${dir}/modules`);
    const prompt = process.env.GPT2_PROMPT ?? "Hello, world! It's";
    const n_new = Number(process.env.GPT2_N_NEW ?? 4);
    const r = await interp.runFile<{ new_ids: number[]; text: string }>(`${dir}/gpt2.json`, { prompt, n_new });
    expect(r.ok).toBe(true);
    if (r.ok && process.env.GPT2_EXPECT) expect(r.value.new_ids).toEqual(JSON.parse(process.env.GPT2_EXPECT));
    if (r.ok) console.log(r.value.text, `(fuel ${r.fuel_used})`);
  }, 24 * 60 * 60 * 1000);
});
