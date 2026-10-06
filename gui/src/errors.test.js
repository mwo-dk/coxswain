import { test } from "node:test";
import assert from "node:assert/strict";
import { cause, failure } from "./errors.js";

test("an error is said in one line, the raw text kept for Details", () => {
  assert.equal(cause("/home/me/budget.txt: Permission denied (os error 13)"), "Permission denied");
  assert.equal(cause("Neither podman nor docker is installed"), "Neither podman nor docker is installed");
  assert.equal(cause("a.txt: locked: wrong password\nb.txt: No space left on device (os error 28)"), "Wrong password");
  assert.deepEqual(failure("Could not copy", "x: gone"), { kind: "message", title: "Could not copy", text: "Gone", details: "x: gone" });
  assert.equal(failure("t", "plain").details, "");
  // Georgian has no capitals: the first letter stays Mkhedruli.
  assert.equal(cause("a.txt: ნებართვა უარყოფილია"), "ნებართვა უარყოფილია");
});
