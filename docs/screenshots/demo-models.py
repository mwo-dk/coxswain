"""Small neural network models for the demo home and tools/preview-check.py, written by hand
(no ONNX, TensorFlow or coremltools needed): an ONNX image classifier, a TensorFlow Lite
model, a Core ML model and package, and a Keras archive. Their weights are made up.

    python3 demo-models.py <folder>
"""

import json, struct, sys, zipfile
from pathlib import Path


# ---------------------------------------------------------------- protocol buffers

def varint(n):
    n &= (1 << 64) - 1
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def vi(field, n):
    return varint(field << 3) + varint(n)


def ld(field, b):
    if isinstance(b, str):
        b = b.encode()
    return varint(field << 3 | 2) + varint(len(b)) + b


def packed(field, nums):
    return ld(field, b"".join(varint(n) for n in nums))


def weights(n, seed):
    """n made-up float32 weights, small and varied, as raw bytes."""
    x, out = seed, []
    for _ in range(n):
        x = (x * 1103515245 + 12345) & 0x7FFFFFFF
        out.append((x / 0x7FFFFFFF - 0.5) * 0.2)
    return struct.pack(f"<{n}f", *out)


# ---------------------------------------------------------------- ONNX

def onnx_model():
    def value(name, dims):
        dim = b"".join(ld(1, vi(1, d) if isinstance(d, int) else ld(2, d)) for d in dims)
        return ld(1, name) + ld(2, ld(1, vi(1, 1) + ld(2, dim)))

    def init(name, dims, seed):
        n = 1
        for d in dims:
            n *= d
        return ld(5, packed(1, dims) + vi(2, 1) + ld(8, name) + ld(9, weights(n, seed)))

    def ints(name, v):
        return ld(5, ld(1, name) + vi(20, 7) + packed(8, v))

    def node(op, ins, outs, name, *attrs):
        return ld(1, b"".join(ld(1, i) for i in ins) + b"".join(ld(2, o) for o in outs) + ld(3, name) + ld(4, op) + b"".join(attrs))

    graph = b"".join([
        node("Conv", ["image", "conv1.weight", "conv1.bias"], ["c1"], "conv1", ints("kernel_shape", [3, 3]), ints("pads", [1, 1, 1, 1])),
        node("Relu", ["c1"], ["r1"], "relu1"),
        node("MaxPool", ["r1"], ["p1"], "pool1", ints("kernel_shape", [2, 2]), ints("strides", [2, 2])),
        node("Conv", ["p1", "conv2.weight", "conv2.bias"], ["c2"], "conv2", ints("kernel_shape", [3, 3]), ints("pads", [1, 1, 1, 1])),
        node("Relu", ["c2"], ["r2"], "relu2"),
        node("MaxPool", ["r2"], ["p2"], "pool2", ints("kernel_shape", [2, 2]), ints("strides", [2, 2])),
        node("Flatten", ["p2"], ["f"], "flatten", ld(5, ld(1, "axis") + vi(20, 2) + vi(3, 1))),
        node("Gemm", ["f", "fc.weight", "fc.bias"], ["logits"], "fc", ld(5, ld(1, "transB") + vi(20, 2) + vi(3, 1))),
        node("Softmax", ["logits"], ["probabilities"], "softmax", ld(5, ld(1, "axis") + vi(20, 2) + vi(3, 1))),
        ld(2, "rocket_vision"),
        init("conv1.weight", [8, 3, 3, 3], 1), init("conv1.bias", [8], 2),
        init("conv2.weight", [16, 8, 3, 3], 3), init("conv2.bias", [16], 4),
        init("fc.weight", [5, 1024], 5), init("fc.bias", [5], 6),
        ld(11, value("image", ["batch", 3, 32, 32])),
        ld(12, value("probabilities", ["batch", 5])),
    ])
    return vi(1, 8) + ld(2, "pytorch") + ld(3, "2.5.1") + ld(8, ld(1, "") + vi(2, 17)) + ld(7, graph)


# ---------------------------------------------------------------- flat buffers (TensorFlow Lite)

class Flat:
    """A flat buffer written front to back: each table's vtable just before it, children after."""

    def __init__(self, ident):
        self.b = bytearray(8)
        self.b[4:8] = ident

    def align(self, n):
        while len(self.b) % n:
            self.b.append(0)

    def table(self, fields):
        # fields: {index: ("u8"|"u32"|"str"|"ints"|"bytes"|"tables", value)}
        n = max(fields) + 1 if fields else 0
        self.align(2)
        vt = len(self.b)
        self.b += bytes(4 + 2 * n)
        self.align(4)
        t = len(self.b)
        self.b += struct.pack("<i", t - vt)
        children = []
        for i, (kind, v) in sorted(fields.items()):
            at = len(self.b)
            if kind == "u8":
                self.b.append(v)
            elif kind == "u32":
                self.b += struct.pack("<I", v)
            else:
                self.b += bytes(4)
                children.append((at, kind, v))
            struct.pack_into("<H", self.b, vt + 4 + 2 * i, at - t)
            self.align(4)
        struct.pack_into("<HH", self.b, vt, 4 + 2 * n, len(self.b) - t)
        for at, kind, v in children:
            self.align(16 if kind == "bytes" else 4)
            if kind == "bytes":
                # The data after its length, both aligned.
                self.b += bytes(12)
            c = len(self.b)
            if kind == "str":
                self.b += struct.pack("<I", len(v)) + v.encode() + b"\0"
            elif kind == "ints":
                self.b += struct.pack(f"<I{len(v)}i", len(v), *v)
            elif kind == "bytes":
                self.b += struct.pack("<I", len(v)) + v
            elif kind == "tables":
                self.b += struct.pack("<I", len(v))
                slots = len(self.b)
                self.b += bytes(4 * len(v))
                for k, tb in enumerate(v):
                    p = self.table(tb)
                    struct.pack_into("<I", self.b, slots + 4 * k, p - (slots + 4 * k))
            struct.pack_into("<I", self.b, at, c - at)
        return t

    def root(self, fields):
        struct.pack_into("<I", self.b, 0, self.table(fields))
        return bytes(self.b)


def tflite_model():
    def tensor(name, shape, buffer):
        return {0: ("ints", shape), 1: ("u8", 0), 2: ("u32", buffer), 3: ("str", name)}

    def op(code, ins, outs):
        return {0: ("u32", code), 1: ("ints", ins), 2: ("ints", outs)}

    tensors = [
        tensor("serving_default_telemetry:0", [1, 16], 0),
        tensor("lander/dense/MatMul", [32, 16], 1),
        tensor("lander/dense/BiasAdd/ReadVariableOp", [32], 2),
        tensor("lander/dense/Relu", [1, 32], 0),
        tensor("lander/thrust/MatMul", [4, 32], 3),
        tensor("lander/thrust/BiasAdd/ReadVariableOp", [4], 4),
        tensor("lander/thrust/BiasAdd", [1, 4], 0),
        tensor("StatefulPartitionedCall:0", [1, 4], 0),
    ]
    sub = {
        0: ("tables", tensors),
        1: ("ints", [0]),
        2: ("ints", [7]),
        3: ("tables", [op(0, [0, 1, 2], [3]), op(0, [3, 4, 5], [6]), op(1, [6], [7])]),
        4: ("str", "main"),
    }
    codes = [{0: ("u8", 9), 2: ("u32", 1), 3: ("u32", 9)}, {0: ("u8", 25), 2: ("u32", 1), 3: ("u32", 25)}]
    buffers = [{}, {0: ("bytes", weights(512, 7))}, {0: ("bytes", weights(32, 8))}, {0: ("bytes", weights(128, 9))}, {0: ("bytes", weights(4, 10))}]
    return Flat(b"TFL3").root({0: ("u32", 3), 1: ("tables", codes), 2: ("tables", [sub]), 3: ("str", "MLIR Converted."), 4: ("tables", buffers)})


# ---------------------------------------------------------------- Core ML

def coreml_model():
    def array(name, dims):
        return ld(1, name) + ld(3, ld(5, packed(1, dims) + vi(2, 65568)))

    def entry(k, v):
        return ld(100, ld(1, k) + ld(2, v))

    desc = (ld(1, array("telemetry", [1, 16])) + ld(10, array("thrust", [1, 4]))
            + ld(100, ld(1, "Thrust from the lander's telemetry") + ld(3, "Rocket team")
                 + entry("com.github.apple.coremltools.version", "8.1") + entry("com.github.apple.coremltools.source", "torch==2.5.1")))

    def dense(name, i, o, ins, outs, seed):
        params = vi(1, i) + vi(2, o) + vi(10, 1) + ld(20, ld(1, weights(i * o, seed))) + ld(21, ld(1, weights(o, seed + 1)))
        return ld(1, ld(1, name) + ld(2, ins) + ld(3, outs) + ld(140, params))

    def relu(name, ins, outs):
        return ld(1, ld(1, name) + ld(2, ins) + ld(3, outs) + ld(130, ld(10, b"")))

    nn = dense("dense", 16, 32, "telemetry", "h", 11) + relu("relu", "h", "hr") + dense("thrust_out", 32, 4, "hr", "thrust", 13)
    return vi(1, 4) + ld(2, desc) + ld(500, nn)


def keras_archive(path):
    config = {
        "module": "keras", "class_name": "Sequential",
        "config": {"name": "lander", "layers": [
            {"module": "keras.layers", "class_name": "InputLayer", "config": {"name": "telemetry", "batch_shape": [None, 16], "dtype": "float32"}},
            {"module": "keras.layers", "class_name": "Dense", "config": {"name": "dense", "units": 32, "activation": "relu"}},
            {"module": "keras.layers", "class_name": "Dense", "config": {"name": "thrust", "units": 4, "activation": "linear"}},
        ]},
        "build_config": {"input_shape": [None, 16]},
    }
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("config.json", json.dumps(config))
        z.writestr("metadata.json", json.dumps({"keras_version": "3.6.0", "date_saved": "2026-09-14@10:12:44"}))


def write(folder):
    d = Path(folder)
    d.mkdir(parents=True, exist_ok=True)
    (d / "rocket-vision.onnx").write_bytes(onnx_model())
    (d / "lander.tflite").write_bytes(tflite_model())
    (d / "Thrust.mlmodel").write_bytes(coreml_model())
    pkg = d / "Thrust.mlpackage"
    (pkg / "Data/com.apple.CoreML").mkdir(parents=True, exist_ok=True)
    (pkg / "Data/com.apple.CoreML/model.mlmodel").write_bytes(coreml_model())
    (pkg / "Manifest.json").write_text(json.dumps({
        "fileFormatVersion": "1.0.0",
        "itemInfoEntries": {"7A2E": {"author": "com.apple.CoreML", "description": "CoreML Model Specification", "name": "model.mlmodel", "path": "com.apple.CoreML/model.mlmodel"}},
        "rootModelIdentifier": "7A2E",
    }, indent=2))
    keras_archive(d / "lander.keras")


if __name__ == "__main__":
    write(sys.argv[1])
