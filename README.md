# ObjScript

A small, sandboxed, statically checked scripting language whose source code is JSON.

ObjScript is built for one job: letting **AI agents write scripts** that get stored in a database and run safely by a host application (originally a Discord bot). JSON makes scripts easy for models to generate, easy to store and diff, and easy to validate. A Rust type checker makes sure nothing ill-typed ever runs.

> **Status:** proof of concept. The language, checker, interpreter, WASM build and Bun package work end to end. Expect breaking changes.

- **Turing complete**: loops, mutable variables, closures.
- **Statically checked**: every script is type-checked before it runs, and all errors are reported at once with JSON paths and fix hints.
- **Sandboxed**: scripts only touch the outside world through capabilities the host grants, and every run has hard limits on steps, call depth, value size and host calls.
- **One engine everywhere**: the Rust core runs natively (CLI) and as WASM (npm/Bun package).

---

## A taste

```json
{
  "objscript": "0.2",
  "description": "GET a URL and return the response body.",
  "imports": { "fetch": "host/http/fetch" },
  "params": { "url": "string" },
  "returns": "json",
  "body": [
    { "let": "res", "value": { "call": "fetch", "args": { "url": { "ref": "url" } } } },
    { "return": { "get": { "ref": "res" }, "at": "body" } }
  ]
}
```

```ts
import { ObjScriptInterpreter, httpFetch } from "objscript";

const interp = new ObjScriptInterpreter({ capabilities: [httpFetch] });
const result = await interp.runFile("./fetch.json", { url: "https://api.example.com/thing" });
// { ok: true, value: {...}, fuel_used: 8, host_calls: 1 }
```

---

## Installing (Bun)

The package is published to the `dist` branch by CI on every `v*` tag.

```json
"dependencies": {
  "objscript": "github:Morgandri1/objscript#dist"
}
```

Pin a specific release with `#dist-vX.Y.Z`. Bun's lockfile pins the exact commit; run `bun update objscript` to pick up a new release.

The package ships TypeScript source and the compiled WASM. It's intended for Bun.

---

## Using it from TypeScript

```ts
import { ObjScriptInterpreter, httpFetch } from "objscript";

// Create once and reuse: compiled scripts are cached.
const interp = new ObjScriptInterpreter({
  capabilities: [httpFetch],
  limits: { fuel: 50_000 },
});

// Library modules: importable by their `name` (and `name@version`) or by file path.
await interp.loadModules(["./modules/weather.json", "./modules/format.json"]);

const result = await interp.runFile("./scripts/weather_cmd.json", { zipCode: "10001" });

if (result.ok) {
  use(result.value);              // whatever the script returned
} else if ("error" in result) {
  console.error(result.error);    // runtime error: out_of_fuel, decode_failed, host_error, ...
} else {
  console.error(result.errors);   // the script failed type checking
}
```

### API

| Method | Purpose |
|---|---|
| `new ObjScriptInterpreter({ capabilities?, limits? })` | `capabilities` is an array of `Capability`; `limits` are defaults for every run |
| `loadModules(files)` | Load module files. Each is importable by its `name`, `name@version` (if it declares `version`), or its path |
| `addModule(source)` | Register an in-memory module (must have a `name`) |
| `check(script, baseDir?)` | Type-check without running → `{ ok: true }` or `{ ok: false, errors }` |
| `run(script, inputs, { limits?, ctx?, baseDir? })` | Run a script object |
| `runFile(path, inputs, { limits?, ctx? })` | Run a script file; relative imports resolve against its folder |

`run` and `runFile` return:

```ts
type RunResult<T> =
  | { ok: true; value: T; fuel_used: number; host_calls: number }
  | { ok: false; error: RuntimeError }      // failed while running
  | { ok: false; errors: Diagnostic[] };    // failed type checking
```

`inputs` are keyed by the script's `params` and validated against their declared types before the script runs. `value` is guaranteed to match the script's declared `returns`.

### Capabilities

A capability is a host function a script can import (`host/...`). Each one declares its contract and a handler:

```ts
import type { Capability } from "objscript";

export const greet: Capability = {
  path: "host/demo/greet",
  params: { name: "string" },
  returns: "string",
  handler: async ({ name }) => `hi ${name}`,
};
```

A capability needs to exist on **both** sides: its signature in the Rust core (which decides what scripts may import) and a handler in TS (which performs the call). If only one side has it, scripts fail with `capability_not_granted` at check time or `no handler registered` at runtime.

The `ctx` option on `run`/`runFile` is passed to every handler, so per-run state (like the current Discord interaction) can reach them.

---

## The language

A script (or library module) is one JSON object:

```jsonc
{
  "objscript": "0.2",                  // language version
  "name": "@org/pkg/thing",            // library modules only
  "version": 1,                        // optional, enables @org/pkg/thing@1 imports
  "description": "...",
  "imports": { "alias": "host/http/fetch" | "@org/pkg/name" | "./file.json" },
  "params": { "name": "string" | { "type": T, "description": "..." } },
  "returns": T,
  "body": [ /* statements */ ],
  "tests": [ /* optional */ ]
}
```

**Encoding rule:** in expression position, JSON strings, numbers, booleans and `null` are literals, arrays are lists, and objects are always single-key nodes. Record literals are written `{"rec": {...}}`, so data can never be mistaken for code.

### Statements

| Node | Meaning |
|---|---|
| `{"let": "x", "value": E}` | Bind a name. Optional `"type": T`, `"mut": true` |
| `{"set": "x", "value": E}` | Reassign a `mut` binding |
| `{"if": E, "then": [...], "else": [...]}` | Conditional (`else` optional) |
| `{"while": E, "do": [...]}` | Loop |
| `{"return": E}` | Return from the function or script |
| `{"do": E}` | Evaluate for side effects |

### Expressions

| Node | Meaning |
|---|---|
| `"s"`, `1`, `1.5`, `true`, `null`, `[...]` | Literals and lists |
| `{"ref": "x"}` | Variable |
| `{"call": "f", "args": {...} \| [...]}` | Call: named args for imports/modules/lambdas, positional for built-ins |
| `{"rec": {"k": E}}` | Record |
| `{"get": E, "at": "field" \| 0}` | Field or index (negative indexes count from the end) |
| `{"and": [...]}`, `{"or": [...]}` | Short-circuit logic |
| `{"fn": {"params": {...}, "returns": T, "body": [...]}}` | Closure (captures by value) |
| `{"decode": E, "as": T}` | Turn untyped `json` into `T`, checked at runtime |

### Types

```
"null" | "bool" | "int" | "float" | "string" | "bytes" | "json"
{"list": T} | {"map": T} | {"option": T} | {"rec": {"field": T}}
{"fn": {"params": {...}, "returns": T}}
```

`json` is untyped data (for example an HTTP body). You can't read fields from it until you `decode` it.

### Built-ins

Always in scope, positional args: `add sub mul div mod eq neq lt lte gt gte not concat to_string len push unwrap_or`. Run `objscript catalog` for signatures.

### Semantics worth knowing

- Call lookup order: local fn variables, then imports, then built-ins.
- The checker normalizes scripts: named args become positional, omitted `option` args become `null`, and int literals become floats where a float is expected.
- Missing map keys and out-of-range indexes are runtime errors; record fields are checked statically.
- Anything returned by a host call is re-validated against its declared type (`host_contract` error).
- Captured variables are read-only inside closures.

### Limits

Every run has a budget. Exceeding any of them stops the run with an error rather than hanging the host.

| Limit | Default | Error |
|---|---|---|
| `fuel`: 1 unit per statement, expression and loop iteration | 100,000 | `out_of_fuel` |
| `max_depth`: nested calls | 64 | `stack_overflow` |
| `max_host_calls` | 16 | `too_many_host_calls` |
| `max_size`: items in a list / bytes in a string | 64 KiB | `too_large` |

Fuel counts steps, not time. Put timeouts on slow capabilities (the bundled `httpFetch` uses one).

### Errors are written for agents

Check errors are structured so a model can fix them in one round:

```json
{
  "path": "/body/0/value/args/zipCode",
  "code": "type_mismatch",
  "message": "expected string, found int",
  "hint": "convert with {\"call\": \"to_string\", \"args\": [...]}"
}
```

The JSON Schema in `schema/objscript.schema.json` gives editors autocomplete (add `"$schema"` to a script) and can be handed to a model as a structured-output schema.

---

## CLI

```sh
cargo run -- catalog                                   # built-ins + host capabilities, as JSON
cargo run -- check examples/broken.json                # every type error at once
cargo run -- run   examples/sum.json --args '{"n": 100}'
cargo run -- run   examples/loop.json                  # stops with out_of_fuel
cargo run -- run   examples/fetch.json --args '{"url": "https://api.example.com"}'
```

Library modules are passed with `--dep IMPORT_PATH=FILE`. All output is JSON.

| Example | Shows |
|---|---|
| `sum.json` | loops and mutation (Turing-completeness check) |
| `fetch.json` | a real HTTP request |
| `loop.json` | an infinite loop stopped by the fuel limit |
| `broken.json` | many errors reported at once |
| `getWeather.json` | a library module |

---

## How async host calls work

The interpreter runs synchronously, but capabilities like `fetch` are async in JS. ObjScript bridges this by **replaying** the run:

1. WASM runs the script until it reaches a host call whose result it doesn't have, then stops and returns the call to JS.
2. JS performs the call (`await`), records the result, and runs the script again from the start, supplying every recorded result in order.
3. This repeats until the script finishes.

This is correct because scripts are deterministic: no clock, no randomness, and every side effect goes through a host call, so replayed calls are never performed twice. The cost is re-running the script once per host call, which is negligible at the default limit of 16. A pausable bytecode VM would remove that cost; see the roadmap.

---

## Repository layout

```
src/                    Rust core (crate `objscript`)
  parse/                JSON -> AST, errors carry JSON Pointers and hints
  check/                type checker; the only way to build a runnable Program
  interp/               interpreter with resource limits
  capability/           capability contracts (e.g. http.rs)
  host/                 Host trait + CLI host
  types/                AST, types, runtime values
  stdlib.rs             pure built-ins
  diag.rs               structured diagnostics
  main.rs               CLI (feature `cli`)
wasm/                   WASM bindings (compile + replay `step`)
ts/                     Bun package: ObjScriptInterpreter, capabilities, tests
  pkg/                  generated by wasm-pack (gitignored)
schema/                 JSON Schema for scripts
examples/               example scripts
scripts/bump.ts         version bump + tag
.github/workflows/      release: build, test, publish to `dist`
```

---

## Development

Requirements: Rust (with the `wasm32-unknown-unknown` target), `wasm-pack`, Bun.

```sh
rustup target add wasm32-unknown-unknown
cargo build && cargo test            # core + CLI

cd ts
bun install
bun run build:wasm                   # builds ../wasm into ts/pkg
bun run typecheck
bun test
```

Don't set a default `build.target` in `.cargo/config.toml`: the CLI must build natively, and `wasm-pack` passes the WASM target itself.

### Releasing

```sh
cd ts
bun run bump:patch        # or bump:minor / bump:major; bumps all versions, commits, tags vX.Y.Z
git push --follow-tags    # CI builds, tests, and publishes to `dist` + tag `dist-vX.Y.Z`
```

`scripts/bump.ts` keeps `Cargo.toml`, `wasm/Cargo.toml` and `ts/package.json` on the same version. The language version (`"objscript": "0.2"`) is separate and only changes when the script format does.

---

## Roadmap

- `fail` and `try`, so scripts can reject bad responses and recover from errors
- Agent tooling: catalog + check + test exposed as model tools, with a generate → check → fix loop
- Golden tests over `examples/`
- Pausable bytecode VM (removes replay overhead, enables saving runs)
- `bytes` literals, a `secret` type that can't leak into output
- A language server built on the JSON Schema and JSON-path diagnostics