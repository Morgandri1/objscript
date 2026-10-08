import { type ObjType } from "../types";

export * from "./std/http"

export interface Capability<C = undefined> {
  path: `${string}/${string}/${string}`;
  params: Record<string, ObjType>;
  returns: ObjType;
  handler: (args: Record<string, unknown>, ctx: C) => unknown | Promise<unknown>;
}