import { describe, expect, test } from "bun:test";
import { httpFetch, ObjScriptInterpreter } from "../src";

const fixture = (name: string) => `${import.meta.dir}/../../examples/${name}`;

describe("ObjScriptInterpreter", () => {
  test("returns the fetched body", async () => {
    const interp = new ObjScriptInterpreter({ capabilities: [httpFetch] });
    const result = await interp.runFile(fixture("fetch.json"), { url: "https://fe-api.avo.so" });
    expect(result).toEqual({ ok: true, value: { error: "Not found" }, fuel_used: 8, host_calls: 1 });
  });

  test("rejects bad inputs", async () => {
    const interp = new ObjScriptInterpreter({ capabilities: [httpFetch] });
    const result = await interp.runFile(fixture("fetch.json"), { url: 5 });
    expect(result.ok).toBe(false);
  });

  test("missing handler is a clear error", async () => {
    const interp = new ObjScriptInterpreter();
    const result = await interp.runFile(fixture("fetch.json"), { url: "https://fe-api.avo.so" });
    expect(result).toMatchObject({ ok: false, error: { code: "host_error" } });
  });
});