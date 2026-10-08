import {type Capability } from ".";

/** Ready-made `host/http/fetch`. */
export const httpFetch: Capability<any> = {
  params: {
    url: "string",
    headers: { option: { map: "string" } },
    query: { option: { map: "string" } }
  },
  returns: {
    rec: { status: "int", body: "json" }
  },
  async handler({ url, headers, query }) {
    const u = new URL(url as string);
    for (const [k, v] of Object.entries((query ?? {}) as Record<string, string>)) u.searchParams.set(k, v);
    const res = await fetch(u, {
      headers: (headers ?? {}) as Record<string, string>,
      signal: AbortSignal.timeout(10_000),
    });
    const text = await res.text();
    let body: unknown;
    try { body = JSON.parse(text); } catch { body = text; }
    return { status: res.status, body };
  },
};