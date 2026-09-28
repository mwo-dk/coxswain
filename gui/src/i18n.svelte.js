// Texts in the user's language. The catalogue comes from bosum-core (see crates/bosum-core/
// src/i18n.rs and locales/), already merged with British English, the reference. Reading
// `i18n` inside t() makes every text reactive: switching the language redraws all of them.

export const i18n = $state({ lang: "en-GB", strings: {}, rtl: false });

let rules = new Intl.PluralRules("en-GB");

/** Use a new language (from get_config / save_settings). */
export function setLanguage(cfg) {
  i18n.lang = cfg.language;
  i18n.strings = cfg.strings;
  i18n.rtl = cfg.rtl;
  rules = new Intl.PluralRules(cfg.language);
  document.documentElement.lang = cfg.language;
  document.documentElement.dir = cfg.rtl ? "rtl" : "ltr";
}

const fill = (s, params) => s.replace(/\{(\w+)\}/g, (m, k) => (k in params ? String(params[k]) : m));

/** The text for `key` with `{name}` placeholders filled. A missing key shows as itself. */
export function t(key, params = {}) {
  const v = i18n.strings[key];
  if (typeof v === "string") return fill(v, params);
  if (v && typeof v === "object") return fill(v.other ?? key, params);
  return key;
}

/** The text for `key` and count `n`, in the plural form the language uses; `{n}` is the count,
 *  formatted for the language (1 234 or 1.234). */
export function tn(key, n, params = {}) {
  const v = i18n.strings[key];
  const all = { n: num(n), ...params };
  if (v && typeof v === "object") return fill(v[rules.select(n)] ?? v.other ?? key, all);
  return typeof v === "string" ? fill(v, all) : key;
}

/** A number formatted for the language. */
export const num = (n, opts) => Number(n).toLocaleString(i18n.lang, opts);
