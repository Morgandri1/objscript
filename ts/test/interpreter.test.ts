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

  test("importing an ungranted capability fails the check", async () => {
    const interp = new ObjScriptInterpreter();
    const result = await interp.runFile(fixture("fetch.json"), { url: "https://fe-api.avo.so" });
    expect(result).toMatchObject({ ok: false, errors: [{ code: "capability_not_granted" }] });
  });

  test("host-declared capabilities are checked and called", async () => {
    const calls: unknown[] = [];
    const interp = new ObjScriptInterpreter<{ guild: string }>({
      capabilities: [{
        path: "acme/discord/addRole",
        params: { userId: "string", roleId: "string", reason: { option: "string" } },
        returns: { rec: { ok: "bool" } },
        handler: (args, ctx) => (calls.push({ ...args, guild: ctx.guild }), { ok: true }),
      }],
    });
    const script = {
      objscript: "0.2",
      imports: { addRole: "acme/discord/addRole" },
      params: { user: "string" },
      returns: "bool",
      body: [{ return: { get: { call: "addRole", args: { userId: { ref: "user" }, roleId: "r1" } }, at: "ok" } }],
    };
    expect(interp.check(script)).toEqual({ ok: true });
    const result = await interp.run(script, { user: "u1" }, { ctx: { guild: "g1" } });
    expect(result).toMatchObject({ ok: true, value: true, host_calls: 1 });
    expect(calls).toEqual([{ userId: "u1", roleId: "r1", reason: null, guild: "g1" }]);

    const bad = { ...script, body: [{ return: { get: { call: "addRole", args: { userId: 5, roleId: "r1" } }, at: "ok" } }] };
    expect(interp.check(bad)).toMatchObject({ ok: false, errors: [{ code: "type_mismatch" }] });
  });

  test("host return values are re-validated against the declared type", async () => {
    const interp = new ObjScriptInterpreter({
      capabilities: [{ path: "acme/x/lies", params: {}, returns: "int", handler: () => "not an int" }],
    });
    const script = { objscript: "0.2", imports: { lies: "acme/x/lies" }, returns: "int", body: [{ return: { call: "lies", args: {} } }] };
    expect(await interp.run(script)).toMatchObject({ ok: false, error: { code: "host_contract" } });
  });
});