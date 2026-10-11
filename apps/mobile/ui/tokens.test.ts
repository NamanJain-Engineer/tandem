import { readFileSync } from "node:fs";
import { resolve as pathResolve } from "node:path";
import { describe, expect, it } from "vitest";
import { color, CSS_VARS, rn } from "./tokens";

const css = readFileSync(pathResolve(import.meta.dirname, "../../../packages/ui/src/styles/tokens.css"), "utf8");

/** The dark theme's value of a custom property, with var() references resolved. */
function resolve(name: string): string {
  const dark = css.slice(css.indexOf(':root, [data-theme="dark"]'), css.indexOf('[data-theme="light"]'));
  const raw = css.slice(0, css.indexOf(':root, [data-theme="dark"]'));
  const find = (block: string, n: string) => new RegExp(`${n}:\\s*([^;]+);`).exec(block)?.[1]?.trim();
  let value = find(dark, name) ?? find(raw, name);
  for (let i = 0; value?.startsWith("var(") && i < 5; i++) {
    const ref = value.slice(4, -1).trim();
    value = find(dark, ref) ?? find(raw, ref);
  }
  if (!value) throw new Error(`no ${name}`);
  return value;
}

describe("F-RM-05 tokens", () => {
  it("F-RM-05.7 every colour matches packages/ui's dark theme", () => {
    for (const [key, name] of Object.entries(CSS_VARS)) {
      expect(color[key as keyof typeof color].toLowerCase(), `${key} (${name})`).toBe(resolve(name).toLowerCase());
    }
  });

  it("converts CSS rgb() with alpha to React Native's rgba()", () => {
    expect(rn("rgb(148 163 184 / 0.14)")).toBe("rgba(148, 163, 184, 0.14)");
    expect(rn("#2dd4bf")).toBe("#2dd4bf");
  });
});
