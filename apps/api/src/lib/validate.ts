import type { Context } from "hono";
import type { z } from "zod";
import { ApiError } from "./problem";

/** Parses a JSON body with zod; failures become 422 VALIDATION problems. */
export async function parseJson<T extends z.ZodType>(c: Context, schema: T): Promise<z.infer<T>> {
  let raw: unknown;
  try {
    raw = await c.req.json();
  } catch {
    throw new ApiError(422, "VALIDATION", "Unprocessable Entity", "The request body must be JSON.");
  }
  const result = schema.safeParse(raw);
  if (!result.success) {
    const issues = result.error.issues.map((i) => ({ path: i.path.join("."), message: i.message }));
    throw new ApiError(422, "VALIDATION", "Unprocessable Entity", issues[0]?.message ?? "Invalid request.", {
      issues,
    });
  }
  return result.data;
}
