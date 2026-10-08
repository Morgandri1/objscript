import type { Capability } from "./capabilities";
import { type Limits, type RunResult, type ReplayEntry, type CheckFailure, type Step, type Source, isFilePath } from "./types";
import { compile, type Compiled } from "../pkg/objscript_wasm";
import { readFile } from "node:fs/promises";
import { dirname, resolve as resolvePath } from "node:path";

export interface RunOptions<C> {
  limits?: Limits;
  /** Passed to every capability handler for this run. */
  ctx?: C;
}

export class ObjScriptInterpreter<C = undefined> {
  /** path -> handler. Must match what the WASM side grants, or checks/runs will fail. */
  private capabilities: Record<string, Capability<C>>;
  /** canonical key (absolute file path, or name for in-memory modules) -> module */
  private modules = new Map<string, { source: Source; baseDir: string }>();
  /** "@org/pkg/name" and "@org/pkg/name@N" -> canonical key */
  private byName = new Map<string, string>();
  private limits: Limits;
  private cache = new Map<string, Compiled>();

  constructor(opts: { capabilities?: Record<string, Capability<C>>; limits?: Limits } = {}) {
    this.capabilities = opts.capabilities ?? {};
    this.limits = opts.limits ?? {};
  }

  /** Load module files. Each becomes importable by its `name` (if any) and by its path. */
  async loadModules(files: string[]): Promise<this> {
    for (const file of files) {
      const abs = resolvePath(file);
      const source = JSON.parse(await readFile(abs, "utf8")) as Source;
      this.register(abs, source, dirname(abs));
    }
    return this;
  }

  /** Register an in-memory module. It must have a `name`. */
  addModule(source: Source): this {
    if (!source.name) throw new Error("addModule: in-memory modules need a `name`");
    this.register(source.name, source, process.cwd());
    return this;
  }

  check(script: Source, baseDir = process.cwd()): { ok: true } | CheckFailure {
    const c = this.compile(script, baseDir);
    return "errors" in c ? c : { ok: true };
  }

  /** Run a script file. Its relative imports resolve against its own folder. */
  async runFile<T = unknown>(
    file: string,
    inputs: Record<string, unknown> = {},
    opts: RunOptions<C> = {},
  ): Promise<RunResult<T>> {
    const abs = resolvePath(file);
    const script = JSON.parse(await readFile(abs, "utf8")) as Source;
    return this.run<T>(script, inputs, { ...opts, baseDir: dirname(abs) });
  }

  /** `inputs` are keyed by the script's `params`; `value` is whatever it returns. */
  async run<T = unknown>(
    script: Source,
    inputs: Record<string, unknown> = {},
    opts: RunOptions<C> & { baseDir?: string } = {},
  ): Promise<RunResult<T>> {
    const compiled = this.compile(script, opts.baseDir ?? process.cwd());
    if ("errors" in compiled) return compiled;

    const inputsJson = JSON.stringify(inputs);
    const limitsJson = JSON.stringify({ ...this.limits, ...opts.limits });
    const replay: ReplayEntry[] = [];

    for (;;) {
      const step = JSON.parse(compiled.step(inputsJson, limitsJson, JSON.stringify(replay))) as Step;
      if (step.type === "done") {
        const { type: _, ...result } = step;
        return result as RunResult<T>;
      }

      const cap = this.capabilities[step.path];
      if (!cap) {
        replay.push({ path: step.path, err: `no handler registered for ${step.path}` });
        continue;
      }
      try {
        replay.push({ path: step.path, ok: (await cap.handler(step.args, opts.ctx as C)) ?? null });
      } catch (e) {
        replay.push({ path: step.path, err: e instanceof Error ? e.message : String(e) });
      }
    }
  }

  private register(key: string, source: Source, baseDir: string) {
    const names = source.name ? [source.name, ...(source.version ? [`${source.name}@${source.version}`] : [])] : [];
    for (const n of names) {
      const existing = this.byName.get(n);
      if (existing && existing !== key) throw new Error(`module name ${n} is defined by both ${existing} and ${key}`);
    }
    this.modules.set(key, { source, baseDir });
    for (const n of names) this.byName.set(n, key);
    this.cache.clear();
  }

  /** Rewrite every module import to its canonical key; unknown ones are left for the checker to report. */
  private resolve(source: Source, baseDir: string): Source {
    if (!source.imports) return source;
    const imports: Record<string, string> = {};
    for (const [alias, spec] of Object.entries(source.imports)) {
      if (spec.startsWith("host/")) imports[alias] = spec;
      else if (isFilePath(spec)) imports[alias] = resolvePath(baseDir, spec);
      else imports[alias] = this.byName.get(spec) ?? spec;
    }
    return { ...source, imports };
  }

  private compile(script: Source, baseDir: string): Compiled | CheckFailure {
    const key = JSON.stringify(this.resolve(script, baseDir));
    const hit = this.cache.get(key);
    if (hit) return hit;

    const deps = Object.fromEntries([...this.modules].map(([k, m]) => [k, this.resolve(m.source, m.baseDir)]));
    try {
      const c = compile(key, JSON.stringify(deps));
      this.cache.set(key, c);
      return c;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      try {
        return JSON.parse(msg) as CheckFailure;
      } catch {
        return { ok: false, errors: [{ path: "", code: "internal", message: msg }] };
      }
    }
  }
}