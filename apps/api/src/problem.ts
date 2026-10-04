import type { Problem } from "@tandem/protocol";

export function problem(status: number, title: string, detail?: string): Problem {
  return {
    type: "about:blank",
    title,
    status,
    detail,
  };
}
