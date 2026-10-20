"""Model stores for the demo home, for the pictures of local models: an Ollama store with two
models, a Hugging Face cache with a safetensors model, a GGUF and a PyTorch pickle, and an LM
Studio folder. The files hold real GGUF and safetensors headers with the shapes of real models;
the weights after them are holes (sparse files), so they take the size of the model and almost
nothing on the disk. Nothing is downloaded.

    python3 demo-local-models.py HOME
"""

import hashlib, json, os, struct, sys
from pathlib import Path

home = Path(sys.argv[1])

# ggml types: (id, elements per block, bytes per block)
F32, F16, Q4_K, Q6_K = (0, 1, 4), (1, 1, 2), (12, 256, 144), (14, 256, 210)


def s(t):
    b = t.encode()
    return struct.pack("<Q", len(b)) + b


def kv(key, v):
    if isinstance(v, str):
        return s(key) + struct.pack("<I", 8) + s(v)
    return s(key) + struct.pack("<I", 4) + struct.pack("<I", v)


def llama(layers, emb, ffn, heads, kv_heads, vocab, head_dim=None):
    """The tensors of a llama-shaped model: name, dims, type."""
    hd = head_dim or emb // heads
    t = [("token_embd.weight", [emb, vocab], Q6_K), ("output_norm.weight", [emb], F32)]
    for i in range(layers):
        b = f"blk.{i}."
        t += [
            (b + "attn_norm.weight", [emb], F32),
            (b + "attn_q.weight", [emb, heads * hd], Q4_K),
            (b + "attn_k.weight", [emb, kv_heads * hd], Q4_K),
            (b + "attn_v.weight", [emb, kv_heads * hd], Q6_K),
            (b + "attn_output.weight", [heads * hd, emb], Q4_K),
            (b + "ffn_norm.weight", [emb], F32),
            (b + "ffn_gate.weight", [emb, ffn], Q4_K),
            (b + "ffn_up.weight", [emb, ffn], Q4_K),
            (b + "ffn_down.weight", [ffn, emb], Q6_K),
        ]
    return t


def gguf(path, arch, name, meta, tensors, template):
    keys = [
        kv("general.architecture", arch),
        kv("general.name", name),
        kv("general.license", meta.pop("license")),
        kv("general.file_type", 15),
        kv("tokenizer.chat_template", template),
    ] + [kv(f"{arch}.{k}", v) for k, v in meta.items()]
    head = b"GGUF" + struct.pack("<IQQ", 3, len(tensors), len(keys)) + b"".join(keys)
    off, info = 0, b""
    for n, dims, (ty, per, size) in tensors:
        info += s(n) + struct.pack("<I", len(dims)) + b"".join(struct.pack("<Q", d) for d in dims)
        info += struct.pack("<IQ", ty, off)
        elems = 1
        for d in dims:
            elems *= d
        off += -(-elems // per) * size
        off = -(-off // 32) * 32
    data = -(-(len(head) + len(info)) // 32) * 32
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "wb") as f:
        f.write(head + info)
        f.truncate(data + off)


def safetensors(path, tensors, dtype=("BF16", 2)):
    header, off = {"__metadata__": {"format": "pt"}}, 0
    for n, shape in tensors:
        elems = 1
        for d in shape:
            elems *= d
        header[n] = {"dtype": dtype[0], "shape": shape, "data_offsets": [off, off + elems * dtype[1]]}
        off += elems * dtype[1]
    h = json.dumps(header, separators=(",", ":")).encode()
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "wb") as f:
        f.write(struct.pack("<Q", len(h)) + h)
        f.truncate(8 + len(h) + off)


TEMPLATE = """{{- bos_token }}
{%- for message in messages %}
<|start_header_id|>{{ message['role'] }}<|end_header_id|>

{{ message['content'] | trim }}<|eot_id|>
{%- endfor %}
{%- if add_generation_prompt %}
<|start_header_id|>assistant<|end_header_id|>
{%- endif %}
"""

# ---------------------------------------------------------------- Ollama
store = home / ".ollama/models"
blobs = store / "blobs"
blobs.mkdir(parents=True, exist_ok=True)


def digest(what):
    return "sha256:" + hashlib.sha256(what.encode()).hexdigest()


def blob(d):
    return blobs / d.replace(":", "-")


def text_blob(what, text):
    d = digest(what)
    blob(d).write_text(text)
    return {"mediaType": "", "digest": d, "size": len(text)}


licence = text_blob("llama-licence", "LLAMA 3.2 COMMUNITY LICENSE AGREEMENT\n")
models = {
    ("llama3.2", "3b"): ("Llama 3.2 3B Instruct", dict(context_length=131072, block_count=28, embedding_length=3072, license="llama3.2", **{"attention.head_count": 24, "attention.head_count_kv": 8}), llama(28, 3072, 8192, 24, 8, 128256), licence),
    ("llama3.2", "1b"): ("Llama 3.2 1B Instruct", dict(context_length=131072, block_count=16, embedding_length=2048, license="llama3.2", **{"attention.head_count": 32, "attention.head_count_kv": 8}), llama(16, 2048, 8192, 32, 8, 128256), licence),
    ("qwen3", "0.6b"): ("Qwen3 0.6B", dict(context_length=40960, block_count=28, embedding_length=1024, license="apache-2.0", **{"attention.head_count": 16, "attention.head_count_kv": 8}), llama(28, 1024, 3072, 16, 8, 151936, head_dim=128), None),
}
for (model, tag), (name, meta, tensors, lic) in models.items():
    weights = digest(f"{model}:{tag}")
    arch = "qwen3" if model == "qwen3" else "llama"
    gguf(blob(weights), arch, name, dict(meta), tensors, TEMPLATE)
    layers = [{"mediaType": "application/vnd.ollama.image.model", "digest": weights, "size": blob(weights).stat().st_size}]
    tpl = text_blob(f"{model}-template", TEMPLATE)
    layers.append(dict(tpl, mediaType="application/vnd.ollama.image.template"))
    if lic:
        layers.append(dict(lic, mediaType="application/vnd.ollama.image.license"))
    params = text_blob(f"{model}:{tag}-params", json.dumps({"stop": ["<|eot_id|>"], "temperature": 0.6}))
    layers.append(dict(params, mediaType="application/vnd.ollama.image.params"))
    config = text_blob(f"{model}:{tag}-config", json.dumps({"model_format": "gguf", "model_family": arch}))
    m = store / "manifests/registry.ollama.ai/library" / model / tag
    m.parent.mkdir(parents=True, exist_ok=True)
    m.write_text(json.dumps({"schemaVersion": 2, "mediaType": "application/vnd.docker.distribution.manifest.v2+json", "config": dict(config, mediaType="application/vnd.docker.container.image.v1+json"), "layers": layers}))

# ---------------------------------------------------------------- Hugging Face
hub = home / ".cache/huggingface/hub"


def hf(repo, files):
    """A repository of the cache: each file a blob, linked from the snapshot by its name."""
    d = hub / ("models--" + repo.replace("/", "--"))
    rev = hashlib.sha1(repo.encode()).hexdigest()
    (d / "refs").mkdir(parents=True, exist_ok=True)
    (d / "refs/main").write_text(rev)
    snap = d / "snapshots" / rev
    snap.mkdir(parents=True, exist_ok=True)
    for name, make in files.items():
        h = hashlib.sha256(f"{repo}/{name}".encode()).hexdigest()
        make(d / "blobs" / h)
        link = snap / name
        if not link.is_symlink():
            link.symlink_to(f"../../blobs/{h}")


def qwen_st(p):
    t = [("model.embed_tokens.weight", [151936, 1024]), ("model.norm.weight", [1024]), ("lm_head.weight", [151936, 1024])]
    for i in range(28):
        b = f"model.layers.{i}."
        t += [(b + "self_attn.q_proj.weight", [2048, 1024]), (b + "self_attn.k_proj.weight", [1024, 1024]), (b + "self_attn.v_proj.weight", [1024, 1024]), (b + "self_attn.o_proj.weight", [1024, 2048]), (b + "mlp.gate_proj.weight", [3072, 1024]), (b + "mlp.up_proj.weight", [3072, 1024]), (b + "mlp.down_proj.weight", [1024, 3072]), (b + "input_layernorm.weight", [1024]), (b + "post_attention_layernorm.weight", [1024])]
    safetensors(p, t)


def write(text):
    def make(p):
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
    return make


def pickle_stub(p):
    # Not a real pickle: a few bytes that start like one. It is never opened anyway.
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "wb") as f:
        f.write(b"PK\x03\x04archive/data.pkl\x80\x02")
        f.truncate(1_421_000_000)


hf("Qwen/Qwen3-0.6B", {"model.safetensors": qwen_st, "config.json": write(json.dumps({"architectures": ["Qwen3ForCausalLM"], "num_hidden_layers": 28})), "tokenizer_config.json": write("{}")})
hf("bartowski/SmolLM2-360M-Instruct-GGUF", {"SmolLM2-360M-Instruct-Q4_K_M.gguf": lambda p: gguf(p, "llama", "SmolLM2 360M Instruct", dict(context_length=8192, block_count=32, embedding_length=960, license="apache-2.0", **{"attention.head_count": 15, "attention.head_count_kv": 5}), llama(32, 960, 2560, 15, 5, 49152), TEMPLATE)})
hf("openai-community/gpt2-large", {"pytorch_model.bin": pickle_stub, "config.json": write("{}")})

# ---------------------------------------------------------------- LM Studio
gguf(home / ".lmstudio/models/lmstudio-community/Qwen3-0.6B-GGUF/Qwen3-0.6B-Q4_K_M.gguf", "qwen3", "Qwen3 0.6B", dict(context_length=40960, block_count=28, embedding_length=1024, license="apache-2.0", **{"attention.head_count": 16, "attention.head_count_kv": 8}), llama(28, 1024, 3072, 16, 8, 151936, head_dim=128), TEMPLATE)
