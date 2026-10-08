# ObjScript (proof of concept)

A JSON-encoded, statically checked, sandboxed scripting language meant to be
written by AI agents, stored in a database, and run by a Discord bot.

```
schema/objscript.schema.json   JSON Schema (editor autocomplete + agent structured output)
src/parse.rs                   JSON -> AST, errors carry JSON Pointers + hints
src/check.rs                   type checker; the only way to build a `Program`
src/interp.rs                  tree-walking interpreter with fuel/depth/size/host-call limits
src/stdlib.rs                  pure built-ins + catalog text
src/host.rs                    `Host` trait: the only way a script touches the world
src/main.rs                    CLI: catalog / check / run / test (JSON output)
examples/                      sum, weather (+ module), infinite loop, broken
```

## Try it

```sh
cargo build

cargo run -- catalog

cargo run -- test  examples/sum.json
cargo run -- run   examples/sum.json --args '{"n": 100}'

cargo run -- check examples/broken.json          # every error, as JSON
cargo run -- run   examples/loop.json            # dies with out_of_fuel

cargo run -- test  examples/weather.json --dep @morgan/tools/getWeather@1=examples/getWeather.json
WEATHER_API_KEY=abc cargo run -- run examples/weather.json \
  --dep @morgan/tools/getWeather@1=examples/getWeather.json --args '{"zipCode": "10001"}'
```

`host/http/fetch` is stubbed in the CLI (returns canned weather) to keep the
crate dependency-free apart from `serde_json`.

## Semantics worth knowing

- Lookup order for calls: local fn variables, then imports, then built-ins.
- The checker rewrites named args to positional, fills omitted `option` args
  with `null`, and turns int literals into floats where a float is expected.
- `get` on a map key or list index that doesn't exist is a runtime error
  (negative indexes count from the end). Record fields are checked statically.
- Anything returned from a host call or mock is re-validated against the
  declared return type (`host_contract` error).
- Closures capture by value; captured variables are read-only inside the fn.

## Not yet

- Bytecode VM with pause/resume on host calls (needed for async + WASM)
- `try`/error values, a `fail` built-in, `bytes` literals
- `Arc` instead of `Rc` so runs are `Send` for tokio
- WASM bindings + npm wrapper
