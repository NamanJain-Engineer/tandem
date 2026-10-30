import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** The scenes' folder, found from this file rather than the working
 * directory, so a build or a test run from anywhere reads the same one. */
const SCENES = join(dirname(fileURLToPath(import.meta.url)), "../components/site/scenes");

/** A scene's own stylesheet (components/site/scenes/<file>), read at build
 * time for the pages that play that scene, so no other page loads it
 * (D-3001). Under `next dev` each render reads it again, so a reload shows
 * an edit. */
export function sceneCss(file: string): string {
  return minifySceneCss(file, readFileSync(join(SCENES, file), "utf8"));
}

/** `css` with its comments gone and its whitespace closed up: each run is
 * one space, and none is left around a brace or a semicolon. A quoted
 * string or a url() can hold whitespace that matters, so neither is taken,
 * but an empty string (a pseudo-element's `content: ""`) holds none; nor is
 * "</", since it's inlined in a <style> (D-3002). */
export function minifySceneCss(file: string, css: string): string {
  const out = css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\s+/g, " ")
    .replace(/\s*([{};])\s*/g, "$1")
    .trim();
  if (/["']|url\(/i.test(out.replaceAll('""', ""))) throw new Error(`scenes: ${file} can't be inlined: it has a quoted string or a url(), whose whitespace closing up could change`);
  if (out.includes("</")) throw new Error(`scenes: ${file} can't be inlined: it has "</"`);
  return out;
}
