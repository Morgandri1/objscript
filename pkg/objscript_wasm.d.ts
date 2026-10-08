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

/**
 * Parse + check a script. Throws an Error whose message is `{"ok":false,"errors":[...]}`.
 * `deps`: `{ "@org/pkg/name@1": <module json>, ... }`
 * `capabilities`: `{ "std/x": { "params": {...}, "returns": T }, ... }`
 */
export function compile(script: string, deps: string): Compiled;
