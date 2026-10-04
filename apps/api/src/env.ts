import { z } from "zod";

/** API configuration from the environment (ARCHITECTURE §17.5). */
const schema = z.object({
  PORT: z.coerce.number().int().positive().default(8787),
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  LOG_LEVEL: z.string().default("info"),
  API_PUBLIC_URL: z.string().url().default("http://api.tandem.local:8787"),
  WEB_PUBLIC_URL: z.string().url().default("http://app.tandem.local:3000"),
});

export type Env = z.infer<typeof schema>;

export function loadEnv(source: Record<string, string | undefined> = process.env): Env {
  return schema.parse(source);
}
