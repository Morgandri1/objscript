#!/usr/bin/env python3
"""Generate GPT-2 as ObjScript: library modules, one module per weight tensor,
the BPE tokenizer, and the entry script. Needs numpy.

  uv run --with numpy examples/gpt2/generate.py \
      --weights model.safetensors --vocab vocab.json --merges merges.txt \
      --out ../objscript-gpt

  uv run --with numpy examples/gpt2/generate.py --tiny --vocab vocab.json --merges merges.txt --out /tmp/tiny

Everything lands in <out>/modules (flat, so `objscript run <out>/gpt2.json -d <out>/modules`
finds it). Weights are int8 (or int16) per output row with f32 scales, embedded as base64
inside modules that return `bytes`; nothing needs file access at runtime.
"""
import argparse, base64, json, os, struct
import numpy as np

# ---------------------------------------------------------------- tiny AST DSL

ref = lambda n: {"ref": n}
call = lambda f, *a: {"call": f, "args": list(a)}
get = lambda t, at: {"get": t, "at": at}
add = lambda a, b: call("add", a, b)
sub = lambda a, b: call("sub", a, b)
mul = lambda a, b: call("mul", a, b)
div = lambda a, b: call("div", a, b)
lt = lambda a, b: call("lt", a, b)
gt = lambda a, b: call("gt", a, b)
eq = lambda a, b: call("eq", a, b)
rec = lambda **kw: {"rec": kw}
LF = {"list": "float"}
# Whole-number float literals ("0.0") don't survive a round trip through JavaScript's JSON
# (it prints them as "0"), so they would turn into ints under the TypeScript host. Build them
# with to_float instead, which is unambiguous everywhere.
F0 = {"call": "to_float", "args": [0]}
F1 = {"call": "to_float", "args": [1]}


def let(n, v, mut=False, ty=None):
    d = {"let": n, "value": v}
    if mut: d["mut"] = True
    if ty is not None: d["type"] = ty
    return d

def set_(n, v): return {"set": n, "value": v}
def store(n, at, v): return {"store": n, "at": at, "value": v}
def ret(v): return {"return": v}
def do(v): return {"do": v}
def while_(c, body): return {"while": c, "do": body}
def if_(c, then, els=None):
    d = {"if": c, "then": then}
    if els is not None: d["else"] = els
    return d

def counted(var, n, body, step=1):
    """let var = 0; while var < n { body; var += step }"""
    return [let(var, 0, mut=True), while_(lt(ref(var), n), body + [set_(var, add(ref(var), step))])]

def module(name, params, returns, body, imports=None, description=None):
    m = {"objscript": "0.2", "name": name}
    if description: m["description"] = description
    if imports: m["imports"] = imports
    m["params"] = params
    m["returns"] = returns
    m["body"] = body
    return m

# ----------------------------------------------------------------- quantization

def quantize(w_out_in, bits):
    """Per-row symmetric quantization. Returns (q int array, scales f32)."""
    qmax = 2 ** (bits - 1) - 1
    amax = np.abs(w_out_in).max(axis=1)
    scale = np.where(amax == 0, 1.0, amax / qmax).astype(np.float32)
    q = np.clip(np.round(w_out_in / scale[:, None]), -qmax, qmax)
    return q.astype({8: np.int8, 16: np.int16}[bits]), scale

def dequantize(q, scale):
    return q.astype(np.float64) * scale.astype(np.float64)[:, None]

def b64(a): return base64.b64encode(np.ascontiguousarray(a).tobytes()).decode()
def f32le(a): return np.asarray(a, dtype="<f4")

# ---------------------------------------------------------------------- weights

def load_safetensors(path):
    with open(path, "rb") as f:
        n = struct.unpack("<Q", f.read(8))[0]
        header = json.loads(f.read(n))
        base = 8 + n
        out = {}
        for k, v in header.items():
            if k == "__metadata__": continue
            assert v["dtype"] == "F32", (k, v["dtype"])
            s, e = v["data_offsets"]
            f.seek(base + s)
            out[k.removeprefix("transformer.")] = np.frombuffer(f.read(e - s), dtype="<f4").reshape(v["shape"]).astype(np.float32)
        return out

def tiny_weights(vocab, d=16, n_layer=2, n_ctx=64, seed=0):
    r = np.random.default_rng(seed)
    n = lambda *s: (r.standard_normal(s) * 0.3).astype(np.float32)
    w = {"wte.weight": n(vocab, d), "wpe.weight": n(n_ctx, d) * 0.3,
         "ln_f.weight": 1 + n(d) * 0.1, "ln_f.bias": n(d) * 0.1}
    for i in range(n_layer):
        p = f"h.{i}."
        for ln in ("ln_1", "ln_2"):
            w[p + ln + ".weight"] = 1 + n(d) * 0.1
            w[p + ln + ".bias"] = n(d) * 0.1
        w[p + "attn.c_attn.weight"] = n(d, 3 * d); w[p + "attn.c_attn.bias"] = n(3 * d) * 0.1
        w[p + "attn.c_proj.weight"] = n(d, d);     w[p + "attn.c_proj.bias"] = n(d) * 0.1
        w[p + "mlp.c_fc.weight"] = n(d, 4 * d);    w[p + "mlp.c_fc.bias"] = n(4 * d) * 0.1
        w[p + "mlp.c_proj.weight"] = n(4 * d, d);  w[p + "mlp.c_proj.bias"] = n(d) * 0.1
    return w

def config(w):
    vocab, d = w["wte.weight"].shape
    n_layer = 1 + max(int(k.split(".")[1]) for k in w if k.startswith("h."))
    return dict(vocab=vocab, d=d, n_layer=n_layer, n_ctx=w["wpe.weight"].shape[0])

# --------------------------------------------------------------- library modules

def lib_vadd():
    return module("@gpt2/vadd", {"a": LF, "b": LF}, LF, [
        let("n", call("len", ref("a"))),
        let("out", call("repeat", F0, ref("n")), mut=True),
        *counted("i", ref("n"), [store("out", ref("i"), add(get(ref("a"), ref("i")), get(ref("b"), ref("i"))))]),
        ret(ref("out")),
    ], description="Elementwise sum of two vectors.")

def lib_linear(bits):
    w = bits // 8
    rd = f"read_i{bits}"
    p, k, base = ref("p"), ref("k"), ref("base")
    terms = []
    for u in range(8):
        idx = p if u == 0 else add(p, u)
        xi = get(ref("x"), k if u == 0 else add(k, u))
        terms.append(mul(call("to_float", call(rd, ref("q"), mul(idx, w))), xi))
    acc = terms[0]
    for t in terms[1:]:
        acc = add(acc, t)
    return module("@gpt2/linear",
        {"x": LF, "q": "bytes", "s": "bytes", "b": "bytes", "n_in": "int", "n_out": "int"}, LF, [
        let("out", call("repeat", F0, ref("n_out")), mut=True),
        *counted("row", ref("n_out"), [
            let("base", mul(ref("row"), ref("n_in"))),
            let("acc", F0, mut=True),
            let("k", 0, mut=True),
            while_(lt(k, ref("n_in")), [
                let("p", add(base, k)),
                set_("acc", add(ref("acc"), acc)),
                set_("k", add(k, 8)),
            ]),
            store("out", ref("row"),
                  add(mul(ref("acc"), call("read_f32", ref("s"), mul(ref("row"), 4))),
                      call("read_f32", ref("b"), mul(ref("row"), 4)))),
        ]),
        ret(ref("out")),
    ], description=f"y = x @ W^T * scale + b, with W stored as int{bits} rows (n_in must be a multiple of 8).")

def lib_layernorm():
    x, n = ref("x"), ref("n")
    return module("@gpt2/layernorm", {"x": LF, "g": "bytes", "b": "bytes", "n": "int"}, LF, [
        let("sum", F0, mut=True),
        *counted("i", n, [set_("sum", add(ref("sum"), get(x, ref("i"))))]),
        let("mean", div(ref("sum"), call("to_float", n))),
        let("var", F0, mut=True),
        *counted("j", n, [
            let("c", sub(get(x, ref("j")), ref("mean"))),
            set_("var", add(ref("var"), mul(ref("c"), ref("c")))),
        ]),
        let("inv", div(F1, call("sqrt", add(div(ref("var"), call("to_float", n)), 1e-5)))),
        let("out", call("repeat", F0, n), mut=True),
        *counted("k", n, [
            store("out", ref("k"),
                  add(mul(mul(sub(get(x, ref("k")), ref("mean")), ref("inv")), call("read_f32", ref("g"), mul(ref("k"), 4))),
                      call("read_f32", ref("b"), mul(ref("k"), 4)))),
        ]),
        ret(ref("out")),
    ], description="LayerNorm with eps 1e-5.")

def lib_gelu():
    v = ref("v")
    inner = mul(0.7978845608028654, add(v, mul(0.044715, mul(v, mul(v, v)))))
    return module("@gpt2/gelu", {"x": LF}, LF, [
        let("n", call("len", ref("x"))),
        let("out", call("repeat", F0, ref("n")), mut=True),
        *counted("i", ref("n"), [
            let("v", get(ref("x"), ref("i"))),
            store("out", ref("i"), mul(mul(0.5, v), add(F1, call("tanh", inner)))),
        ]),
        ret(ref("out")),
    ], description="GELU, tanh approximation (gelu_new).")

def lib_softmax():
    x, n = ref("x"), ref("n")
    return module("@gpt2/softmax", {"x": LF}, LF, [
        let("n", call("len", x)),
        let("m", sub(F0, call("inf")), mut=True),
        *counted("i", n, [if_(gt(get(x, ref("i")), ref("m")), [set_("m", get(x, ref("i")))])]),
        let("out", call("repeat", F0, n), mut=True),
        let("sum", F0, mut=True),
        *counted("j", n, [
            let("e", call("exp", sub(get(x, ref("j")), ref("m")))),
            store("out", ref("j"), ref("e")),
            set_("sum", add(ref("sum"), ref("e"))),
        ]),
        *counted("k", n, [store("out", ref("k"), div(get(ref("out"), ref("k")), ref("sum")))]),
        ret(ref("out")),
    ], description="Numerically stable softmax.")

def lib_attention():
    d = mul(ref("n_head"), ref("head_dim"))
    all_k, all_v, off, T = ref("all_k"), ref("all_v"), ref("off"), ref("T")
    return module("@gpt2/attention",
        {"q": LF, "k": LF, "v": LF, "ks": {"list": LF}, "vs": {"list": LF}, "n_head": "int", "head_dim": "int"},
        LF, [
        let("all_k", call("push", ref("ks"), ref("k"))),
        let("all_v", call("push", ref("vs"), ref("v"))),
        let("T", call("len", all_k)),
        let("scale", div(F1, call("sqrt", call("to_float", ref("head_dim"))))),
        let("out", call("repeat", F0, d), mut=True),
        *counted("h", ref("n_head"), [
            let("off", mul(ref("h"), ref("head_dim"))),
            let("scores", call("repeat", F0, T), mut=True),
            *counted("t", T, [
                let("kt", get(all_k, ref("t"))),
                let("acc", F0, mut=True),
                *counted("j", ref("head_dim"), [
                    set_("acc", add(ref("acc"), mul(get(ref("q"), add(off, ref("j"))), get(ref("kt"), add(off, ref("j")))))),
                ]),
                store("scores", ref("t"), mul(ref("acc"), ref("scale"))),
            ]),
            let("w", call("softmax", ref("scores"))),
            *counted("j2", ref("head_dim"), [
                let("acc2", F0, mut=True),
                *counted("t2", T, [
                    set_("acc2", add(ref("acc2"), mul(get(ref("w"), ref("t2")), get(get(all_v, ref("t2")), add(off, ref("j2")))))),
                ]),
                store("out", add(off, ref("j2")), ref("acc2")),
            ]),
        ]),
        ret(ref("out")),
    ], imports={"softmax": "@gpt2/softmax"}, description="Causal multi-head attention for one new position over a KV cache.")

# ------------------------------------------------------------------ model graph

def wname(n): return "@gpt2/w/" + n

def block(i, cfg):
    d, nh = cfg["d"], cfg["n_head"]
    hd = d // nh
    wi = lambda s: f"h{i}_{s}"
    imports = {"layernorm": "@gpt2/layernorm", "linear": "@gpt2/linear", "attention": "@gpt2/attention",
               "gelu": "@gpt2/gelu", "vadd": "@gpt2/vadd"}
    for s in ("ln_1", "attn_c_attn", "attn_c_proj", "ln_2", "mlp_c_fc", "mlp_c_proj"):
        imports["w_" + s] = wname(wi(s))
    lin = lambda src, w, n_in, n_out: call("linear", src, get(ref(w), "q"), get(ref(w), "s"), get(ref(w), "b"), n_in, n_out)
    ln = lambda src, w: call("layernorm", src, get(ref(w), "g"), get(ref(w), "b"), d)
    body = [
        let("n1", call("w_ln_1")), let("wa", call("w_attn_c_attn")), let("wp", call("w_attn_c_proj")),
        let("n2", call("w_ln_2")), let("wf", call("w_mlp_c_fc")), let("wm", call("w_mlp_c_proj")),
        let("h", ln(ref("x"), "n1")),
        let("qkv", lin(ref("h"), "wa", d, 3 * d)),
        let("q", call("slice", ref("qkv"), 0, d)),
        let("k", call("slice", ref("qkv"), d, 2 * d)),
        let("v", call("slice", ref("qkv"), 2 * d, 3 * d)),
        let("a", call("attention", ref("q"), ref("k"), ref("v"), ref("ks"), ref("vs"), nh, hd)),
        let("ap", lin(ref("a"), "wp", d, d)),
        let("x1", call("vadd", ref("x"), ref("ap"))),
        let("h2", ln(ref("x1"), "n2")),
        let("f", call("gelu", lin(ref("h2"), "wf", d, 4 * d))),
        let("m", lin(ref("f"), "wm", 4 * d, d)),
        ret(rec(x=call("vadd", ref("x1"), ref("m")), k=ref("k"), v=ref("v"))),
    ]
    return module(f"@gpt2/block{i}", {"x": LF, "ks": {"list": LF}, "vs": {"list": LF}},
                  {"rec": {"x": LF, "k": LF, "v": LF}}, body, imports,
                  f"Transformer block {i}: returns the new hidden state and this position's k and v.")

def wte_chunks(cfg):
    """[(start_row, end_row)] of the embedding table, split so no module file exceeds GitHub's 100 MB limit."""
    R, V = cfg["wte_chunk_rows"], cfg["vocab"]
    return [(a, min(a + R, V)) for a in range(0, V, R)]

def embed(cfg, bits):
    d, w, R = cfg["d"], bits // 8, cfg["wte_chunk_rows"]
    chunks = wte_chunks(cfg)
    empty = rec(q=call("bytes_new", 0), s=call("bytes_new", 0))
    return module("@gpt2/embed", {"id": "int", "pos": "int"}, LF, [
        let("p", call("wpe")),
        let("c", div(ref("id"), R)),
        let("t", empty, mut=True),
        *[if_(eq(ref("c"), i), [set_("t", call(f"wte{i}"))]) for i in range(len(chunks))],
        let("q", get(ref("t"), "q")), let("s", get(ref("t"), "s")),
        let("local", sub(ref("id"), mul(ref("c"), R))),
        let("sc", call("read_f32", ref("s"), mul(ref("local"), 4))),
        let("row", mul(ref("local"), d)), let("prow", mul(ref("pos"), d)),
        let("out", call("repeat", F0, d), mut=True),
        *counted("i", d, [
            store("out", ref("i"),
                  add(mul(call("to_float", call(f"read_i{bits}", ref("q"), mul(add(ref("row"), ref("i")), w))), ref("sc")),
                      call("read_f32", get(ref("p"), "b"), mul(add(ref("prow"), ref("i")), 4)))),
        ]),
        ret(ref("out")),
    ], imports={**{f"wte{i}": wname(f"wte_{i}") for i in range(len(chunks))}, "wpe": wname("wpe")},
       description="Token + position embedding. The token table is split into row chunks; only the chunk holding the id is loaded.")

def head(cfg):
    d = cfg["d"]
    chunks = wte_chunks(cfg)
    body = [
        let("f", call("lnf")),
        let("h", call("layernorm", ref("x"), get(ref("f"), "g"), get(ref("f"), "b"), d)),
        let("out", call("repeat", F0, 0), mut=True),
    ]
    for i, (a, b) in enumerate(chunks):
        body += [
            let(f"t{i}", call(f"wte{i}")),
            set_("out", call("concat_lists", ref("out"),
                             call("linear", ref("h"), get(ref(f"t{i}"), "q"), get(ref(f"t{i}"), "s"),
                                  call("bytes_new", (b - a) * 4), d, b - a))),
        ]
    body.append(ret(ref("out")))
    return module("@gpt2/head", {"x": LF}, LF, body,
        imports={**{f"wte{i}": wname(f"wte_{i}") for i in range(len(chunks))},
                 "lnf": wname("ln_f"), "layernorm": "@gpt2/layernorm", "linear": "@gpt2/linear"},
        description="Final LayerNorm and the tied output projection, chunk by chunk: logits over the vocabulary.")

# ---------------------------------------------------------------------- tokenizer

def bytes_to_unicode():
    bs = list(range(33, 127)) + list(range(161, 173)) + list(range(174, 256))
    cs, n = bs[:], 0
    for b in range(256):
        if b not in bs:
            bs.append(b); cs.append(256 + n); n += 1
    m = dict(zip(bs, map(chr, cs)))
    return [m[b] for b in range(256)]

PATTERN = r"'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+"

def tokenizer_modules(vocab_json, merges_txt):
    vocab = json.load(open(vocab_json, encoding="utf8"))
    toks = [None] * len(vocab)
    for t, i in vocab.items(): toks[i] = t
    assert all(t is not None and "\n" not in t for t in toks)
    merges = [l for l in open(merges_txt, encoding="utf8").read().split("\n")[1:] if l]
    b2u = bytes_to_unicode()

    def table(name, text, desc):
        return module(name, {}, "string", [ret(text)], description=desc)

    def index_map(var, src, key_of=lambda e: e):
        return [let(var, {"rec": {}}, mut=True, ty={"map": "int"}),
                *counted("i_" + var, call("len", ref(src)), [store(var, key_of(get(ref(src), ref("i_" + var))), ref("i_" + var))])]

    r = ref
    bpe_loop = [
        let("go", True, mut=True),
        while_(r("go"), [
            let("n", call("len", r("parts"))),
            let("best", -1, mut=True), let("best_rank", 1_000_000_000, mut=True), let("i", 0, mut=True),
            while_(lt(r("i"), sub(r("n"), 1)), [
                let("key", call("concat", get(r("parts"), r("i")), " ", get(r("parts"), add(r("i"), 1)))),
                if_(call("has", r("ranks"), r("key")), [
                    let("rk", get(r("ranks"), r("key"))),
                    if_(lt(r("rk"), r("best_rank")), [set_("best_rank", r("rk")), set_("best", r("i"))]),
                ]),
                set_("i", add(r("i"), 1)),
            ]),
            if_(lt(r("best"), 0), [set_("go", False)], [
                let("a", get(r("parts"), r("best"))), let("b", get(r("parts"), add(r("best"), 1))),
                let("merged", call("repeat", "", 0), mut=True),
                let("j", 0, mut=True),
                while_(lt(r("j"), r("n")), [
                    if_({"and": [lt(r("j"), sub(r("n"), 1)), eq(get(r("parts"), r("j")), r("a")),
                                 eq(get(r("parts"), add(r("j"), 1)), r("b"))]},
                        [set_("merged", call("push", r("merged"), call("concat", r("a"), r("b")))), set_("j", add(r("j"), 2))],
                        [set_("merged", call("push", r("merged"), get(r("parts"), r("j")))), set_("j", add(r("j"), 1))]),
                ]),
                set_("parts", r("merged")),
            ]),
        ]),
    ]
    encode = module("@gpt2/encode", {"text": "string"}, {"list": "int"}, [
        let("b2u", b2u),
        let("toks", call("split", call("vocab"), "\n")),
        *index_map("vid", "toks"),
        let("lines", call("split", call("merges"), "\n")),
        *index_map("ranks", "lines"),
        let("pieces", call("regex_find_all", r("text"), PATTERN)),
        let("ids", call("repeat", 0, 0), mut=True),
        *counted("pi", call("len", r("pieces")), [
            let("bs", call("bytes_to_list", call("to_bytes", get(r("pieces"), r("pi"))))),
            let("parts", call("repeat", "", 0), mut=True),
            *counted("bi", call("len", r("bs")), [
                set_("parts", call("push", r("parts"), get(r("b2u"), get(r("bs"), r("bi")))))]),
            *bpe_loop,
            *counted("ti", call("len", r("parts")), [
                set_("ids", call("push", r("ids"), get(r("vid"), get(r("parts"), r("ti")))))]),
        ]),
        ret(r("ids")),
    ], imports={"vocab": wname("vocab"), "merges": wname("merges")},
       description="GPT-2 byte-level BPE: text to token ids.")

    decode = module("@gpt2/decode", {"ids": {"list": "int"}}, "string", [
        let("b2u", b2u),
        let("toks", call("split", call("vocab"), "\n")),
        *index_map("u2b", "b2u"),
        let("bs", call("repeat", 0, 0), mut=True),
        *counted("i", call("len", r("ids")), [
            let("cs", call("chars", get(r("toks"), get(r("ids"), r("i"))))),
            *counted("c", call("len", r("cs")), [
                set_("bs", call("push", r("bs"), get(r("u2b"), get(r("cs"), r("c")))))]),
        ]),
        ret(call("from_utf8_lossy", call("bytes_from_list", r("bs")))),
    ], imports={"vocab": wname("vocab")}, description="GPT-2 byte-level BPE: token ids to text.")

    data = [table(wname("vocab"), "\n".join(toks), "Token strings in id order, one per line."),
            table(wname("merges"), "\n".join(merges), "BPE merge pairs, best first, one per line.")]
    return [encode, decode] + data

# -------------------------------------------------------------------- entry script

def main_script(cfg, progress=False):
    L = cfg["n_layer"]
    r = ref
    imports = {"encode": "@gpt2/encode", "decode": "@gpt2/decode", "embed": "@gpt2/embed", "head": "@gpt2/head"}
    if progress:
        imports["print"] = "std/io/print"
    for i in range(L):
        imports[f"block{i}"] = f"@gpt2/block{i}"
    per_layer = []
    for i in range(L):
        per_layer += [
            let(f"r{i}", call(f"block{i}", r("x"), get(r("kc"), i), get(r("vc"), i))),
            set_("x", get(r(f"r{i}"), "x")),
            store("kc", i, call("push", get(r("kc"), i), get(r(f"r{i}"), "k"))),
            store("vc", i, call("push", get(r("vc"), i), get(r(f"r{i}"), "v"))),
        ]
    step = [
        let("x", call("embed", get(r("ids"), r("pos")), r("pos")), mut=True),
        *per_layer,
        if_(call("gte", r("pos"), sub(r("n_prompt"), 1)), [
            let("logits", call("head", r("x"))),
            let("best", 0, mut=True), let("bv", get(r("logits"), 0), mut=True), let("i", 1, mut=True),
            while_(lt(r("i"), call("len", r("logits"))), [
                let("v", get(r("logits"), r("i"))),
                if_(gt(r("v"), r("bv")), [set_("bv", r("v")), set_("best", r("i"))]),
                set_("i", add(r("i"), 1)),
            ]),
            set_("ids", call("push", r("ids"), r("best"))),
            set_("gen", add(r("gen"), 1)),
            *([do(call("print", call("concat", "token ", r("gen"), ": ", r("best"), " ", call("decode", [r("best")]))))] if progress else []),
        ]),
        set_("pos", add(r("pos"), 1)),
    ]
    return {
        "objscript": "0.2",
        "description": f"GPT-2 ({L} layers, d={cfg['d']}) greedy text generation, written entirely in ObjScript.",
        "imports": imports,
        "params": {"prompt": "string", "n_new": "int"},
        "returns": {"rec": {"text": "string", "ids": {"list": "int"}, "new_ids": {"list": "int"}}},
        "body": [
            let("ids", call("encode", r("prompt")), mut=True),
            let("n_prompt", call("len", r("ids"))),
            let("empty", [], ty={"list": LF}),
            let("kc", call("repeat", r("empty"), L), mut=True),
            let("vc", call("repeat", r("empty"), L), mut=True),
            let("pos", 0, mut=True), let("gen", 0, mut=True),
            while_({"and": [lt(r("pos"), call("len", r("ids"))), lt(r("gen"), r("n_new"))]}, step),
            ret(rec(text=call("decode", r("ids")), ids=r("ids"), new_ids=call("slice", r("ids"), r("n_prompt"), call("len", r("ids"))))),
        ],
    }

# ------------------------------------------------------------------------ driver

def weight_modules(w, bits, wte_chunk_rows):
    """name -> module for every tensor."""
    mods = []
    def mat(name, w_in_out, bias):
        q, s = quantize(np.asarray(w_in_out, dtype=np.float64).T, bits)
        mods.append(module(wname(name), {}, {"rec": {"q": "bytes", "s": "bytes", "b": "bytes"}},
                           [ret(rec(q={"call": "from_base64", "args": [b64(q)]},
                                    s={"call": "from_base64", "args": [b64(f32le(s))]},
                                    b={"call": "from_base64", "args": [b64(f32le(bias))]}))]))
    def ln(name, g, b):
        mods.append(module(wname(name), {}, {"rec": {"g": "bytes", "b": "bytes"}},
                           [ret(rec(g={"call": "from_base64", "args": [b64(f32le(g))]},
                                    b={"call": "from_base64", "args": [b64(f32le(b))]}))]))
    cfg = config(w); cfg["wte_chunk_rows"] = wte_chunk_rows
    for i, (a, b) in enumerate(wte_chunks(cfg)):
        q, s = quantize(w["wte.weight"][a:b].astype(np.float64), bits)
        mods.append(module(wname(f"wte_{i}"), {}, {"rec": {"q": "bytes", "s": "bytes"}},
                           [ret(rec(q={"call": "from_base64", "args": [b64(q)]}, s={"call": "from_base64", "args": [b64(f32le(s))]}))],
                           description=f"Token embedding rows {a}..{b - 1}, int{bits} with per-row f32 scales."))
    mods.append(module(wname("wpe"), {}, {"rec": {"b": "bytes"}},
                       [ret(rec(b={"call": "from_base64", "args": [b64(f32le(w["wpe.weight"]))]}))]))
    ln("ln_f", w["ln_f.weight"], w["ln_f.bias"])
    for i in range(cfg["n_layer"]):
        p = f"h.{i}."
        ln(f"h{i}_ln_1", w[p + "ln_1.weight"], w[p + "ln_1.bias"])
        ln(f"h{i}_ln_2", w[p + "ln_2.weight"], w[p + "ln_2.bias"])
        mat(f"h{i}_attn_c_attn", w[p + "attn.c_attn.weight"], w[p + "attn.c_attn.bias"])
        mat(f"h{i}_attn_c_proj", w[p + "attn.c_proj.weight"], w[p + "attn.c_proj.bias"])
        mat(f"h{i}_mlp_c_fc", w[p + "mlp.c_fc.weight"], w[p + "mlp.c_fc.bias"])
        mat(f"h{i}_mlp_c_proj", w[p + "mlp.c_proj.weight"], w[p + "mlp.c_proj.bias"])
    return mods

def write(path, obj):
    with open(path, "w", encoding="utf8") as f:
        json.dump(obj, f, ensure_ascii=False, separators=(",", ":"))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--weights"); ap.add_argument("--tiny", action="store_true")
    ap.add_argument("--vocab", required=True); ap.add_argument("--merges", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--bits", type=int, choices=(8, 16), default=8)
    ap.add_argument("--n-head", type=int, default=None)
    ap.add_argument("--wte-chunk-rows", type=int, default=8192,
                    help="rows of the token table per module (8192 keeps every file under ~17 MB at int16)")
    ap.add_argument("--progress", action="store_true",
                    help="print each token as it is generated (CLI only: needs the std/io/print capability, "
                         "and under the async TypeScript host every host call replays the whole run)")
    a = ap.parse_args()
    if a.tiny:
        w = tiny_weights(50257); n_head = a.n_head or 2
    else:
        w = load_safetensors(a.weights); n_head = a.n_head or 12
    cfg = config(w); cfg["n_head"] = n_head; cfg["wte_chunk_rows"] = a.wte_chunk_rows
    assert cfg["d"] % 8 == 0 and cfg["d"] % n_head == 0
    md = os.path.join(a.out, "modules"); os.makedirs(md, exist_ok=True)
    mods = [lib_vadd(), lib_linear(a.bits), lib_layernorm(), lib_gelu(), lib_softmax(), lib_attention(),
            embed(cfg, a.bits), head(cfg)] + [block(i, cfg) for i in range(cfg["n_layer"])]
    mods += tokenizer_modules(a.vocab, a.merges) + weight_modules(w, a.bits, a.wte_chunk_rows)
    for m in mods:
        write(os.path.join(md, m["name"].lstrip("@").replace("/", "_") + ".json"), m)
    write(os.path.join(a.out, "gpt2.json"), main_script(cfg, a.progress))
    print(f"wrote {len(mods)} modules + gpt2.json to {a.out} ({cfg})")

if __name__ == "__main__":
    main()
