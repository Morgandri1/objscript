#!/usr/bin/env python3
"""NumPy reference for the generated ObjScript model, to check it against.

  uv run --with numpy --with tiktoken examples/gpt2/reference.py --tiny --prompt "Hello" --n-new 5
  uv run --with numpy --with tiktoken examples/gpt2/reference.py --weights model.safetensors --prompt "Hello" --n-new 5
  ... --fp32   use the original unquantized weights (to see what quantization costs)
"""
import argparse, importlib.util, json, os
import numpy as np
import tiktoken

here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("gen", os.path.join(here, "generate.py"))
gen = importlib.util.module_from_spec(spec); spec.loader.exec_module(gen)

def gelu(x): return 0.5 * x * (1 + np.tanh(np.sqrt(2 / np.pi) * (x + 0.044715 * x ** 3)))
def ln(x, g, b): m = x.mean(); v = ((x - m) ** 2).mean(); return (x - m) / np.sqrt(v + 1e-5) * g + b
def softmax(x): e = np.exp(x - x.max()); return e / e.sum()

def effective(w, bits, fp32):
    """Weights the ObjScript model actually computes with."""
    if fp32: return {k: v.astype(np.float64) for k, v in w.items()}
    out = {}
    for k, v in w.items():
        v = v.astype(np.float64)
        if k == "wte.weight":
            out[k] = gen.dequantize(*gen.quantize(v, bits))
        elif k.endswith(".weight") and v.ndim == 2 and "wpe" not in k:
            out[k] = gen.dequantize(*gen.quantize(v.T, bits)).T
        else:
            out[k] = v.astype(np.float32).astype(np.float64)   # biases/LN/wpe are stored as f32
    return out

def run(w, n_head, ids, n_new):
    L = 1 + max(int(k.split(".")[1]) for k in w if k.startswith("h."))
    d = w["wte.weight"].shape[1]; hd = d // n_head
    kc = [[] for _ in range(L)]; vc = [[] for _ in range(L)]
    ids = list(ids); n_prompt = len(ids); pos = 0; gen_n = 0
    while pos < len(ids) and gen_n < n_new:
        x = w["wte.weight"][ids[pos]] + w["wpe.weight"][pos]
        for i in range(L):
            p = f"h.{i}."
            h = ln(x, w[p + "ln_1.weight"], w[p + "ln_1.bias"])
            qkv = h @ w[p + "attn.c_attn.weight"] + w[p + "attn.c_attn.bias"]
            q, k, v = qkv[:d], qkv[d:2 * d], qkv[2 * d:]
            kc[i].append(k); vc[i].append(v)
            K, V = np.array(kc[i]), np.array(vc[i]); a = np.zeros(d)
            for hh in range(n_head):
                s = slice(hh * hd, (hh + 1) * hd)
                a[s] = softmax(K[:, s] @ q[s] / np.sqrt(hd)) @ V[:, s]
            x = x + a @ w[p + "attn.c_proj.weight"] + w[p + "attn.c_proj.bias"]
            h = ln(x, w[p + "ln_2.weight"], w[p + "ln_2.bias"])
            x = x + gelu(h @ w[p + "mlp.c_fc.weight"] + w[p + "mlp.c_fc.bias"]) @ w[p + "mlp.c_proj.weight"] + w[p + "mlp.c_proj.bias"]
        if pos >= n_prompt - 1:
            logits = ln(x, w["ln_f.weight"], w["ln_f.bias"]) @ w["wte.weight"].T
            ids.append(int(np.argmax(logits))); gen_n += 1
        pos += 1
    return ids

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--weights"); ap.add_argument("--tiny", action="store_true")
    ap.add_argument("--prompt", default="Hello"); ap.add_argument("--n-new", type=int, default=5)
    ap.add_argument("--bits", type=int, default=8); ap.add_argument("--fp32", action="store_true")
    ap.add_argument("--n-head", type=int, default=None)
    a = ap.parse_args()
    w = gen.tiny_weights(50257) if a.tiny else gen.load_safetensors(a.weights)
    n_head = a.n_head or (2 if a.tiny else 12)
    enc = tiktoken.get_encoding("gpt2")
    prompt_ids = enc.encode(a.prompt)
    ids = run(effective(w, a.bits, a.fp32), n_head, prompt_ids, a.n_new)
    print(json.dumps({"prompt_ids": prompt_ids, "ids": ids, "new_ids": ids[len(prompt_ids):], "text": enc.decode(ids)}, ensure_ascii=False))
