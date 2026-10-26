// node --test (npm test): the desktop app's table preview (SheetJS, as renderers.js reads a
// workbook) on the real `.xlsb` files the Rust suite downloads (crates/coxswain-core/tests/
// real_files.rs). Opt-in, after that suite: COXSWAIN_REAL_FILES=1, the files in
// COXSWAIN_REAL_FILES_DIR (default ../target/tmp/real-files). Skipped otherwise.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.env.COXSWAIN_REAL_FILES_DIR || "../target/tmp/real-files";
const on = process.env.COXSWAIN_REAL_FILES && existsSync(dir);

test("every real .xlsb shows its sheets as tables", { skip: !on && "COXSWAIN_REAL_FILES is not set, or no files downloaded" }, async () => {
  const XLSX = await import("xlsx");
  const files = readdirSync(dir).filter((f) => f.endsWith(".xlsb"));
  assert.ok(files.length > 0, `no .xlsb in ${dir}`);
  for (const f of files) {
    const wb = XLSX.read(readFileSync(join(dir, f)), { type: "buffer", sheetRows: 201, dense: true });
    assert.ok(wb.SheetNames.length > 0, `${f}: no sheets`);
    let cells = 0;
    for (const name of wb.SheetNames) {
      const rows = XLSX.utils.sheet_to_json(wb.Sheets[name], { header: 1, raw: false, defval: "" });
      cells += rows.flat().filter((c) => String(c).trim()).length;
    }
    assert.ok(cells > 0, `${f}: every sheet empty`);
    console.log(`${f}: ${wb.SheetNames.length} sheets, ${cells} cells in the first rows`);
  }
});
