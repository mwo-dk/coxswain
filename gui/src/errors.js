// An error as a dialog shows it: a title that says what failed, one line of cause, the raw text
// under Details. The terminal app does the same with coxswain_core::fs::cause.

/** The cause in one line: the first line, without the path before it and the OS error number. */
export function cause(err) {
  const first = String(err).split("\n")[0].replace(/\s*\(os error \d+\)$/, "").trim();
  const tail = first.slice(first.lastIndexOf(": ") + 2).trim();
  return first.includes(": ") && tail ? tail[0].toUpperCase() + tail.slice(1) : first;
}

/** The message dialog for `err`, under `title`. */
export function failure(title, err) {
  const raw = String(err).trim();
  const text = cause(raw);
  return { kind: "message", title, text, details: raw === text ? "" : raw };
}
