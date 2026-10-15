/* tslint:disable */
/* eslint-disable */

export class Compiled {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns `{"type":"done", ...result}` or `{"type":"suspend","path":..,"args":{..}}`.
     * `replay`: `[{ "path": ..., "ok": value } | { "path": ..., "err": message }, ...]`
     */
    step(args: string, limits: string, replay: string): string;
}

export function builtins(): string;

/**
 * Parse + check a script. Throws an Error whose message is `{"ok":false,"errors":[...]}`.
 * `deps`: `{ "@org/pkg/name@1": <module json>, ... }`
 * `capabilities`: `{ "org/area/name": { "params": {...}, "returns": T, "description"?: "..." }, ... }`.
 * These are the only capabilities the script may import; param order is call order.
 */
export function compile(script: string, deps: string, capabilities: string): Compiled;
