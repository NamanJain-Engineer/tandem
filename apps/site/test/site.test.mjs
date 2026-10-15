import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const html = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../index.html"), "utf8");

assert.match(html, /Vibezeo/);
assert.match(html, /Claude Code/);
assert.match(html, /contact@vibezeo\.com/);
assert.doesNotMatch(html, /hello@vibezeo\.com/);
assert.match(html, /does not read your Claude login/);
assert.doesNotMatch(html, /\$[0-9]|ARR|customers love|10,000 users|Series [A-D]/i);

console.log("site copy ok");
