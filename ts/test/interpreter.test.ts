import { describe, expect, test } from "bun:test";
import { httpFetch, ObjScriptInterpreter } from "../src";
import type { Capability } from "../src/capabilities";

const fixture = (name: string) => `${import.meta.dir}/../../examples/${name}`;

describe("ObjScriptInterpreter", () => {
  test("returns the fetched body", async () => {
    const interp = new ObjScriptInterpreter({ capabilities: [httpFetch] });
    const result = await interp.runFile(fixture("fetch.json"), { url: "https://example.com" });
    expect(result).toEqual({ ok: true, value: { hello: "world" }, fuel_used: expect.any(Number), host_calls: 1 });
  });

  test("rejects bad inputs", async () => {
    const interp = new ObjScriptInterpreter({ capabilities: [httpFetch] });
    const result = await interp.runFile(fixture("fetch.json"), { url: 5 });
    expect(result.ok).toBe(false);
  });

  test("missing handler is a clear error", async () => {
    const interp = new ObjScriptInterpreter();
    const result = await interp.runFile(fixture("fetch.json"), { url: "https://example.com" });
    expect(result).toMatchObject({ ok: false, error: { code: "host_error" } });
  });
});