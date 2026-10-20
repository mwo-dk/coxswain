// What every Netron module Coxswain leaves out is replaced by (build.mjs): Python and its
// pickles, PyTorch and the other formats, Node's fs and zlib. Nothing here runs a file.
const why = "This part of Netron is not in Coxswain: it does not read Python pickles or code.";

export class Execution {
  constructor() {
    throw new globalThis.Error(why);
  }
}
class PythonError extends globalThis.Error {}
export { PythonError as Error };
export class ModelFactory {
  async match() {
    return null;
  }
}
export default {};
