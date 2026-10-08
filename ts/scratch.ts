import { ObjScriptInterpreter } from "./src/index.ts";
import { httpFetch } from "./src/capabilities/index.ts";

const interp = new ObjScriptInterpreter({ capabilities: { "host/http/fetch": httpFetch } });
console.log(await interp.runFile("../examples/fetch.json", { url: "https://fe-api.avo.so/healthz" }));