export type ObjType =
  | "null" | "bool" | "int" | "float" | "string" | "bytes" | "json"
  | { list: ObjType } | { map: ObjType } | { option: ObjType }
  | { rec: Record<string, ObjType> }
  | { fn: { params: Record<string, ObjType>; returns: ObjType } };

export interface Limits { fuel?: number; max_depth?: number; max_host_calls?: number; max_size?: number }
export interface Diagnostic { path: string; code: string; message: string; hint?: string }
export interface RuntimeError { path: string; code: string; message: string }

/** Same shape as the CLI's `run` output, plus check errors. */
export type RunResult<T = unknown> =
  | { ok: true; value: T; fuel_used: number; host_calls: number }
  | { ok: false; error: RuntimeError }
  | { ok: false; errors: Diagnostic[] };

export type CheckFailure = { ok: false; errors: Diagnostic[] };
export type Step =
  | ({ type: "done" } & RunResult)
  | { type: "suspend"; path: string; args: Record<string, unknown> };
export type ReplayEntry = { path: string; ok: unknown } | { path: string; err: string };
export type Source = { name?: string; version?: number; imports?: Record<string, string>;[k: string]: unknown };
export const isFilePath = (s: string) => s.startsWith("/") || s.startsWith("./") || s.startsWith("../");