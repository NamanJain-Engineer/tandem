/** Joins class names, skipping falsy values (as @tandem/ui's cx, without
 * importing the whole component index into the site's pages). */
export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
