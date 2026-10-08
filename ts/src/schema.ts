import { z } from "zod";
import type { ObjType } from "./types";

const IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/;
const MODULE_NAME = /^@[a-z0-9-]+(\/[A-Za-z0-9_-]+)+$/;
const IMPORT =
  /^(host\/[a-z0-9_]+(\/[a-z0-9_]+)*|@[a-z0-9-]+(\/[A-Za-z0-9_-]+)+(@[0-9]+)?|(\/|\.\/|\.\.\/).+\.json)$/;

export const Ident = z.string().regex(IDENT, "letters, digits and _, not starting with a digit");

export type Expr =
  | string | number | boolean | null
  | Expr[]
  | { ref: string }
  | { call: string; args?: Expr[] | Record<string, Expr> }
  | { rec: Record<string, Expr> }
  | { get: Expr; at: string | number }
  | { and: Expr[] }
  | { or: Expr[] }
  | { fn: { params: Record<string, ObjType>; returns: ObjType; body: Stmt[] } }
  | { decode: Expr; as: ObjType };

export type Stmt =
  | { let: string; mut?: boolean; type?: ObjType; value: Expr }
  | { set: string; value: Expr }
  | { if: Expr; then: Stmt[]; else?: Stmt[] }
  | { while: Expr; do: Stmt[] }
  | { return: Expr }
  | { do: Expr };

export const ObjTypeSchema: z.ZodType<ObjType> = z.lazy(() =>
  z.union([
    z.enum(["null", "bool", "int", "float", "string", "bytes", "json"]),
    z.strictObject({ list: ObjTypeSchema }),
    z.strictObject({ map: ObjTypeSchema }),
    z.strictObject({ option: ObjTypeSchema }),
    z.strictObject({ rec: z.record(z.string(), ObjTypeSchema) }),
    z.strictObject({ fn: z.strictObject({ params: z.record(Ident, ObjTypeSchema), returns: ObjTypeSchema }) }),
  ]),
).describe('A type. Primitives are strings; compound types are single-key objects like {"list": "int"}.');

export const ExprSchema: z.ZodType<Expr> = z.lazy(() =>
  z.union([
    z.union([z.string(), z.number(), z.boolean(), z.null()]).describe("Literal value"),
    z.array(ExprSchema).describe("List literal"),
    z.strictObject({ ref: Ident }).describe("Variable reference"),
    z.strictObject({
      call: Ident,
      args: z.union([z.array(ExprSchema), z.record(Ident, ExprSchema)]).optional(),
    }).describe("Call. Named args (object) for imports/modules/lambdas; positional (array) for built-ins."),
    z.strictObject({ rec: z.record(z.string(), ExprSchema) }).describe("Record literal"),
    z.strictObject({ get: ExprSchema, at: z.union([z.string(), z.number().int()]) }).describe("Field or index access"),
    z.strictObject({ and: z.array(ExprSchema).min(2) }),
    z.strictObject({ or: z.array(ExprSchema).min(2) }),
    z.strictObject({
      fn: z.strictObject({ params: z.record(Ident, ObjTypeSchema), returns: ObjTypeSchema, body: z.array(StmtSchema) }),
    }).describe("Closure"),
    z.strictObject({ decode: ExprSchema, as: ObjTypeSchema }).describe("Give untyped json a type; checked at runtime"),
  ]),
).describe("Expression. JSON primitives are literals, arrays are lists, objects are single-key nodes.");

export const StmtSchema: z.ZodType<Stmt> = z.lazy(() =>
  z.union([
    z.strictObject({ let: Ident, mut: z.boolean().optional(), type: ObjTypeSchema.optional(), value: ExprSchema }),
    z.strictObject({ set: Ident, value: ExprSchema }),
    z.strictObject({ if: ExprSchema, then: z.array(StmtSchema), else: z.array(StmtSchema).optional() }),
    z.strictObject({ while: ExprSchema, do: z.array(StmtSchema) }),
    z.strictObject({ return: ExprSchema }),
    z.strictObject({ do: ExprSchema }).describe("Evaluate for side effects"),
  ]),
);

const ParamSchema = z.union([
  ObjTypeSchema,
  z.strictObject({ type: ObjTypeSchema, description: z.string().optional() }),
]);

const TestSchema = z.strictObject({
  name: z.string().optional(),
  args: z.record(z.string(), z.unknown()),
  mocks: z.record(z.string(), z.unknown()).optional(),
  expect: z.unknown().optional(),
  expectCalls: z.array(z.record(z.string(), z.unknown())).optional(),
});

export const ScriptSchema = z.strictObject({
  $schema: z.string().optional(),
  objscript: z.literal("0.2"),
  name: z.string().regex(MODULE_NAME).optional()
    .describe("Library modules only, e.g. @org/pkg/name. Omit for commands."),
  version: z.number().int().min(1).optional(),
  description: z.string().optional().describe("What this script does."),
  command: z.strictObject({
    name: z.string().regex(/^[-_a-z0-9]{1,32}$/),
    description: z.string().max(100).optional(),
  }).optional().describe("Makes this script a Discord slash command. Options come from params."),
  imports: z.record(Ident, z.string().regex(IMPORT)).optional()
    .describe("alias -> host/... capability, @org/pkg/name module, or path. Built-ins need no import."),
  params: z.record(Ident, ParamSchema).optional().describe("Named inputs. Key order is positional order."),
  returns: ObjTypeSchema,
  body: z.array(StmtSchema),
  tests: z.array(TestSchema).optional(),
});

export type Script = z.infer<typeof ScriptSchema>;