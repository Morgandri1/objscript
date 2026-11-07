import { describe, expect, test } from "bun:test";
import { ObjScriptInterpreter } from "../src";

const run = async (body: unknown[], returns: unknown, params: Record<string, unknown> = {}, args = {}, limits?: object) =>
  new ObjScriptInterpreter().run({ objscript: "0.2", params, returns, body } as any, args, { limits });

describe("builtins", () => {
  test("math", async () => {
    const r = await run([{ return: { rec: {
      e: { call: "exp", args: [1.0] }, p: { call: "pow", args: [2.0, 10.0] }, t: { call: "to_int", args: [3.9] },
      m: { call: "max", args: [3, 9] }, s: { call: "shl", args: [1, 40] },
    } } }], { rec: { e: "float", p: "float", t: "int", m: "int", s: "int" } });
    expect(r).toMatchObject({ ok: true, value: { p: 1024, t: 3, m: 9, s: 2 ** 40 } });
    expect((r as any).value.e).toBeCloseTo(Math.E, 12);
  });

  test("bytes: base64, typed reads", async () => {
    const f32 = Buffer.from(new Float32Array([1.5, -2]).buffer).toString("base64");
    const r = await run([
      { let: "b", value: { call: "from_base64", args: [f32] } },
      { return: { rec: {
        a: { call: "read_f32", args: [{ ref: "b" }, 0] }, b: { call: "read_f32", args: [{ ref: "b" }, 4] },
        n: { call: "len", args: [{ ref: "b" }] }, rt: { call: "to_base64", args: [{ ref: "b" }] },
      } } },
    ], { rec: { a: "float", b: "float", n: "int", rt: "string" } });
    expect(r).toMatchObject({ ok: true, value: { a: 1.5, b: -2, n: 8, rt: f32 } });
  });

  test("regex", async () => {
    const r = await run([{ return: { rec: {
      words: { call: "regex_find_all", args: ["it's 42", "\\p{L}+|\\d+"] },
      sw: { call: "regex_replace", args: ["a-b", "(\\w)-(\\w)", "$2-$1"] },
      look: { call: "regex_find_all", args: ["a  b", "\\s+(?!\\S)"] },
    } } }], { rec: { words: { list: "string" }, sw: "string", look: { list: "string" } } });
    expect(r).toMatchObject({ ok: true, value: { words: ["it", "s", "42"], sw: "b-a", look: [" "] } });
  });

  test("regex errors are runtime errors, not crashes", async () => {
    const r = await run([{ return: { call: "regex_test", args: ["x", "("] } }], "bool");
    expect(r).toMatchObject({ ok: false, error: { code: "builtin_error" } });
  });
});

describe("store and computed keys", () => {
  test("in-place list and map writes with computed indexes", async () => {
    const r = await run([
      { let: "xs", mut: true, type: { list: "int" }, value: { call: "repeat", args: [0, 4] } },
      { let: "m", mut: true, type: { map: "int" }, value: { rec: {} } },
      { let: "i", mut: true, value: 0 },
      { while: { call: "lt", args: [{ ref: "i" }, 4] }, do: [
        { store: "xs", at: { ref: "i" }, value: { call: "mul", args: [{ ref: "i" }, { ref: "i" }] } },
        { set: "i", value: { call: "add", args: [{ ref: "i" }, 1] } },
      ] },
      { store: "m", at: "k", value: 7 },
      { return: { rec: { xs: { ref: "xs" }, k: { get: { ref: "m" }, at: "k" }, last: { get: { ref: "xs" }, at: { call: "sub", args: [{ call: "len", args: [{ ref: "xs" }] }, 1] } } } } },
    ], { rec: { xs: { list: "int" }, k: "int", last: "int" } });
    expect(r).toMatchObject({ ok: true, value: { xs: [0, 1, 4, 9], k: 7, last: 9 } });
  });

  test("storing into an immutable binding is a check error", async () => {
    const r = await run([
      { let: "xs", type: { list: "int" }, value: { call: "repeat", args: [0, 2] } },
      { store: "xs", at: 0, value: 1 },
      { return: null },
    ], "null");
    expect(r).toMatchObject({ ok: false, errors: [{ code: "immutable" }] });
  });
});

describe("limits", () => {
  const spin = [{ let: "i", mut: true, value: 0 }, { while: { call: "lt", args: [{ ref: "i" }, 200000] }, do: [{ set: "i", value: { call: "add", args: [{ ref: "i" }, 1] } }] }, { return: { ref: "i" } }];

  test("default limits stop a long loop", async () => {
    expect(await run(spin, "int")).toMatchObject({ ok: false, error: { code: "out_of_fuel" } });
  });

  test("null lifts fuel", async () => {
    expect(await run(spin, "int", {}, {}, { fuel: null })).toMatchObject({ ok: true, value: 200000 });
  });

  test("max_size can be lifted too", async () => {
    const big = [{ return: { call: "len", args: [{ call: "repeat", args: [0, 100000] }] } }];
    expect(await run(big, "int")).toMatchObject({ ok: false, error: { code: "too_large" } });
    expect(await run(big, "int", {}, {}, { max_size: null })).toMatchObject({ ok: true, value: 100000 });
  });
});
