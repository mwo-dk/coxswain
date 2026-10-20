// Coxswain's host for Netron's view (docs/previews/neural-networks.md). It stands in for
// Netron's browser host (browser.js, not shipped): no telemetry, no update check, no web
// requests, no worker, no file dialog. It runs in a frame sandboxed to scripts only, with no
// origin and a policy that allows no connection; the app sends it the model's bytes, and the
// files a model names next to it (weights, external data), by message. `build.mjs` bundles it
// with the Netron modules of the formats Coxswain opens; every other module is left out.

import * as base from "netron/base.js";
import * as view from "netron/view.js";
import * as onnx from "netron/onnx.js";
import * as onnxProto from "netron/onnx-proto.js";
import * as onnxSchema from "netron/onnx-schema.js";
import * as tflite from "netron/tflite.js";
import * as tfliteSchema from "netron/tflite-schema.js";
import * as coreml from "netron/coreml.js";
import * as coremlProto from "netron/coreml-proto.js";
import * as keras from "netron/keras.js";
import * as kerasProto from "netron/keras-proto.js";
import onnxMetadata from "netron/onnx-metadata.json";
import tfliteMetadata from "netron/tflite-metadata.json";
import coremlMetadata from "netron/coreml-metadata.json";
import kerasMetadata from "netron/keras-metadata.json";

const MODULES = {
  "./onnx": onnx,
  "./onnx-proto": onnxProto,
  "./onnx-schema": onnxSchema,
  "./tflite": tflite,
  "./tflite-schema": tfliteSchema,
  "./coreml": coreml,
  "./coreml-proto": coremlProto,
  "./keras": keras,
  "./keras-proto": kerasProto,
};
const ASSETS = {
  "onnx-metadata.json": onnxMetadata,
  "tflite-metadata.json": tfliteMetadata,
  "coreml-metadata.json": coremlMetadata,
  "keras-metadata.json": kerasMetadata,
};

/** A format Coxswain leaves out (PyTorch, pickles, …): it matches no file. */
const NONE = { ModelFactory: class { async match() { return null; } } };

const app = window.parent;
const tell = (message) => app.postMessage({ netron: true, ...message }, "*");
let asked = 0;
const waiting = new Map();

class Host {
  constructor() {
    window.eval = () => { throw new Error("window.eval() not supported."); };
    this._settings = new Map();
  }
  get window() { return window; }
  get document() { return document; }
  get version() { return NETRON_VERSION; }
  get type() { return "Coxswain"; }
  environment(name) {
    return { name: "Netron", serial: true, menu: false, packaged: false, repository: "" }[name];
  }
  async view(v) { this._view = v; }
  async start() { tell({ ready: true }); }
  async require(id) { return MODULES[id] ?? NONE; }
  async asset(file) {
    if (!(file in ASSETS)) throw new Error(`'${file}' is not part of Coxswain's Netron.`);
    return ASSETS[file];
  }
  /** A file next to the model, read by the app (inside the model's folder only). */
  async fetch(file, encoding) {
    const id = ++asked;
    const bytes = await new Promise((ok, fail) => {
      waiting.set(id, { ok, fail });
      tell({ fetch: file, id });
    });
    const data = new Uint8Array(bytes);
    return encoding ? new TextDecoder(encoding).decode(data) : new base.BinaryStream(data);
  }
  // Links (an operator's documentation) are not followed: a page opened for the frame could
  // carry what it was handed off the machine.
  openURL() {}
  exception(error) { console.error(error); }
  event() {}
  update() {}
  async execute() {}
  async export() {}
  // Settings live only as long as the frame: storage needs an origin.
  get(name) { return this._settings.get(name); }
  set(name, value) { this._settings.set(name, value); }
  delete(name) { this._settings.delete(name); }
  async message(message) {
    tell({ error: message || "" });
  }
}

class Context {
  constructor(host, identifier, stream) {
    this._host = host;
    this._identifier = identifier;
    this._stream = stream;
  }
  get identifier() { return this._identifier; }
  get name() { return this._identifier; }
  get stream() { return this._stream; }
  async asset(file) { return this._host.asset(file); }
  async fetch(file, encoding) { return this._host.fetch(file, encoding); }
  async require(id) { return this._host.require(id); }
  error(error, fatal) { this._host.exception(error, fatal); }
}

const host = new Host();
const netron = new view.View(host);

window.addEventListener("message", async (e) => {
  if (e.source !== app || !e.data || typeof e.data !== "object") return;
  const m = e.data;
  if (m.id && waiting.has(m.id)) {
    const w = waiting.get(m.id);
    waiting.delete(m.id);
    m.bytes ? w.ok(m.bytes) : w.fail(new Error(m.error || "not found"));
    return;
  }
  if (m.open && m.bytes instanceof ArrayBuffer) {
    try {
      const context = new Context(host, String(m.open), new base.BinaryStream(new Uint8Array(m.bytes)));
      const model = await netron.open(context);
      if (!model) throw new Error("No model.");
      netron.show(null);
      tell({ shown: true });
    } catch (error) {
      tell({ error: error?.message ?? String(error) });
    }
  }
});

netron.start();
