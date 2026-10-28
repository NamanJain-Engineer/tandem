import { readFileSync } from "node:fs";
import { join } from "node:path";

/** A scene's own stylesheet (components/site/scenes/<file>), read at build
 * time for the pages that play that scene, so no other page loads it
 * (D-3001). Its comments go and its whitespace closes up. */
export function sceneCss(file: string): string {
  const css = readFileSync(join(process.cwd(), "components/site/scenes", file), "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\s+/g, " ")
    .replace(/\s*([{};])\s*/g, "$1")
    .trim();
  // Inlined in a <style>, so it mustn't close one.
  if (css.includes("</")) throw new Error(`scenes: ${file} can't be inlined: it has "</"`);
  return css;
}
