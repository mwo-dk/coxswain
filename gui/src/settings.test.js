import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// Settings.svelte is drawn by hand; coxswain_core::settings says which area and group each
// option is in. These hold the two together: the same areas in the same order, every option
// shown in its own area, and each group's heading there.
const core = readFileSync(new URL("../../crates/coxswain-core/src/settings.rs", import.meta.url), "utf8");
const svelte = readFileSync(new URL("./Settings.svelte", import.meta.url), "utf8");

const snake = (area) => area.toLowerCase();
const groups = [...core.matchAll(/group\((\w+), "(\w+)", (None|Some\("([\w.]+)"\)), &\[([^\]]*)\]\)/g)].map((m) => ({
  area: snake(m[1]),
  id: m[2],
  head: m[4] ?? null,
  options: [...m[5].matchAll(/"(\w+)"/g)].map((x) => x[1]),
}));

/** The markup of `area` in Settings.svelte: from its `{:else if area === "…"}` to the next. */
function block(area) {
  const start = svelte.indexOf(`{:else if area === "${area}"}`);
  assert.ok(start >= 0, `no block for ${area}`);
  const next = svelte.indexOf("{:else if area === ", start + 1);
  return svelte.slice(start, next < 0 ? undefined : next);
}

test("the desktop app has core's areas, in its order", () => {
  const all = core.match(/pub const ALL: \[Area; \d+\] = \[([^\]]*)\]/)[1];
  const areas = [...all.matchAll(/Area::(\w+)/g)].map((m) => snake(m[1]));
  const ours = JSON.parse(svelte.match(/const AREAS = (\[[^\]]*\]);/)[1]);
  assert.deepEqual(ours, areas);
});

test("every option is shown in its own area, under its group's heading", () => {
  assert.ok(groups.length > 10, "the groups are read");
  for (const g of groups) {
    // Looks draws its own way (the language list, one block for both themes).
    if (g.area === "looks") continue;
    const b = block(g.area);
    for (const o of g.options) assert.ok(b.includes(`"${o}"`), `${o} is not shown in ${g.area}`);
    // Search's groups are headed in Details, Privacy's one has no heading.
    if (g.head && g.area !== "search") assert.ok(b.includes(`t("${g.head}")`), `${g.area}: no heading ${g.head}`);
  }
});
