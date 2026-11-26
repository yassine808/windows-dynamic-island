// Design guardrails for the island chrome: one palette, one type scale, one motion vocabulary,
// readable text, no emoji. Run with `node scripts/design-check.mjs` (also in CI).
import { readFileSync } from "node:fs";

const css = readFileSync("src/styles.css", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
let failures = 0;

const check = (name, ok, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
  if (!ok) failures++;
};

// 1. Colors come from the iOS system palette, nothing ad-hoc.
const palette = new Set(["#000", "#000000", "#fff", "#ffffff", "#ff453a", "#ff8a80", "#ff375f", "#bf5af2", "#0a84ff", "#30d158", "#ff9f0a"]);
const hexes = [...css.matchAll(/#[0-9a-fA-F]{3,8}\b/g)].map((m) => m[0].toLowerCase());
const offPalette = [...new Set(hexes.filter((h) => !palette.has(h)))];
check("palette: every hex color is a system color", offPalette.length === 0, offPalette.join(", "));

// 2. One type scale for the whole island.
const scale = new Set(["11", "11.5", "12", "12.5", "13", "14", "15", "16", "20", "24", "26"]);
const sizes = [];
for (const m of css.matchAll(/font-size:\s*(\d+(?:\.\d+)?)px/g)) sizes.push(m[1]);
for (const m of css.matchAll(/font:\s*[^;]*?(\d+(?:\.\d+)?)px/g)) sizes.push(m[1]);
const offScale = [...new Set(sizes.filter((s) => !scale.has(s)))];
check("type: every font size is on the scale", offScale.length === 0, offScale.join(", "));

// 3. Motion uses the two house curves (or a plain CSS keyword / no timing function).
const allowedCurves = new Set(["cubic-bezier(0.2, 0.8, 0.2, 1)", "cubic-bezier(0.34, 1.56, 0.64, 1)"].map((c) => c.replace(/\s+/g, "")));
const curves = [...css.matchAll(/cubic-bezier\([^)]*\)/g)].map((m) => m[0].replace(/\s+/g, ""));
const wildCurves = [...new Set(curves.filter((c) => !allowedCurves.has(c)))];
check("motion: no stray easing curves", wildCurves.length === 0, wildCurves.join(", "));
check("motion: no steps() animations", !/steps\(/.test(css));
check("motion: reduced motion is honoured", /@media\s*\(prefers-reduced-motion:\s*reduce\)/.test(css));

// 4. Housekeeping.
check("css: no !important", !/!important/.test(css));
check("text: no emoji in the UI", !/[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/u.test(app));

// 5. The type stack is the system one.
const font = css.match(/--font:\s*([^;]+);/)?.[1] ?? "";
check("type: system font stack", /system-ui|Segoe UI Variable/.test(font), font.trim());

// 6. Text stays readable on the black island (WCAG AA for body text).
const lum = (hex) => {
  const n = hex.replace("#", "");
  const c = [0, 2, 4].map((i) => parseInt(n.slice(i, i + 2), 16) / 255);
  const lin = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
};
const contrast = (a, b) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};
// --label-secondary is rgba(235,235,245,0.6) over black -> blended value
const blended = "#" + [235, 235, 245].map((v) => Math.round(v * 0.6).toString(16).padStart(2, "0")).join("");
const secondary = contrast(blended, "#000000");
check("contrast: secondary text on black >= 4.5:1", secondary >= 4.5, secondary.toFixed(2));
check("contrast: primary text on black >= 4.5:1", contrast("#ffffff", "#000000") >= 4.5);

console.log(failures ? `\n${failures} design check(s) failed` : "\nall design checks passed");
process.exit(failures ? 1 : 0);
