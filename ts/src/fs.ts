import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import type { ObjScriptInterpreter } from "./interpreter";
import type { RunOptions } from "./interpreter";
import type { RunResult, Source } from "./types";

/** Helper to read a JSON file as a source, keyed by its absolute path so path imports work. */
export async function readSource(file: string) {
  const key = resolve(file);
  const source = JSON.parse(await readFile(key, "utf8")) as Source;
  return { source, key, baseDir: dirname(key) };
}

/** Loads modules from filepaths */
export async function addModuleFiles<C>(interp: ObjScriptInterpreter<C>, files: string[]) {
  for (const file of files) {
    const { source, ...opts } = await readSource(file);
    interp.addModule(source, opts);
  }
  return interp;
}

/** Run standalone file */
export async function runFile<C, T = unknown>(
  interp: ObjScriptInterpreter<C>,
  file: string,
  inputs: Record<string, unknown> = {},
  opts: RunOptions<C> = {},
): Promise<RunResult<T>> {
  const { source, baseDir } = await readSource(file);
  return interp.run<T>(source, inputs, { ...opts, baseDir });
}