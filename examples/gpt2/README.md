# GPT-2 small in ObjScript

The full GPT-2 small forward pass (12 layers, d=768, 124M parameters), BPE tokenizer
included, written as ObjScript JSON. It is inspired by
[GPT-2 in pure CSS](https://shubhampatil.dev/blog/gpt2-css). No tensor primitives and no
file access: the language's loops, floats, `bytes` and `store` do all of the work, and the
weights ship as ordinary modules that return `bytes`.

`generate.py` writes everything; the generated weights are ~320 MB, so they live outside
this repo (by default in `../objscript-gpt`).

```sh
# 1. fetch the weights and vocab (https://huggingface.co/openai-community/gpt2)
# 2. generate (needs numpy; `uv run --with numpy` works)
uv run --with numpy examples/gpt2/generate.py --bits 16 \
    --weights model.safetensors --vocab vocab.json --merges merges.txt --out ../objscript-gpt
# 3. run it (release build recommended)
cargo build --release
./target/release/objscript run ../objscript-gpt/gpt2.json -d ../objscript-gpt/modules \
    --arg prompt="Once upon a time" --arg n_new=8
```

It returns `{text, ids, new_ids}` (generate with `--progress` to also print each token as it
appears; CLI only). Decoding is greedy. Run it from a folder containing `modules/` and `-d` is
optional. Expect minutes per token: a token is about 125M multiply-adds, each a handful of
interpreter steps.

## Layout (`<out>/`)

| Module | What it is |
|---|---|
| `gpt2.json` | entry script: encode, embed, 12 blocks with a KV cache, head, argmax, decode |
| `@gpt2/linear`, `layernorm`, `gelu`, `softmax`, `attention`, `vadd` | the math, as plain loops |
| `@gpt2/block0..11` | one transformer block each, importing its own weights |
| `@gpt2/embed`, `@gpt2/head` | token+position embedding, final LayerNorm and tied output projection |
| `@gpt2/encode`, `@gpt2/decode` | byte-level BPE using the native `regex_find_all` with GPT-2's pattern |
| `@gpt2/w/*` | one module per tensor; each returns `bytes` built with `from_base64` |

## Weights

Matrices are stored per output row as signed ints with an f32 scale (`--bits 8` or `16`).
Biases, LayerNorm parameters and position embeddings are plain f32. Reading is
`read_i16` / `read_f32` on the `bytes` value.

int8 halves the size (~165 MB) but drifts from fp32 within a few tokens ("Once upon a time,
the world's most powerful…" instead of "…was a place of great…"), which matches the CSS
article's finding that 8 bits is too few. int16 reproduces the fp32 greedy output.

## Checking it

`reference.py` runs the same model in NumPy (float64, using the same quantized weights, so
the comparison is exact rather than approximate) with `tiktoken` for tokenization:

```sh
uv run --with numpy --with tiktoken examples/gpt2/reference.py \
    --weights model.safetensors --bits 16 --prompt "Once upon a time" --n-new 8
```

`generate.py --tiny` builds a random 2-layer, d=16 model with the real tokenizer; its
output matches the reference token for token and runs in about two seconds.
