import { type ObjType } from "../types";

export * from "./http"

export interface Capability<C = unknown> {
  /** Key order is the positional order. */
  params: Record<string, ObjType>;
  returns: ObjType;
  handler: (args: Record<string, unknown>, ctx: C) => unknown | Promise<unknown>;
}