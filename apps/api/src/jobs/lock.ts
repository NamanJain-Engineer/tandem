import type { Redis } from "ioredis";
import { randomToken } from "../lib/ids";

// Releases the lock only when it's still ours.
const RELEASE = `if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) else return 0 end`;

/**
 * Runs `fn` under a Redis lock (SET NX PX) so one replica runs a job at a
 * time (ARCHITECTURE §14.1). Returns null when another holder has it.
 */
export async function withLock<T>(
  redis: Redis,
  key: string,
  ttlMs: number,
  fn: () => Promise<T>,
): Promise<T | null> {
  const token = randomToken(16);
  const ok = await redis.set(key, token, "PX", ttlMs, "NX");
  if (ok !== "OK") return null;
  try {
    return await fn();
  } finally {
    await redis.eval(RELEASE, 1, key, token).catch(() => undefined);
  }
}
