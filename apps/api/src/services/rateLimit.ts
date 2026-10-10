import type { Redis } from "ioredis";

// Atomic token bucket: refill `capacity` tokens per `windowMs`, take one.
const SCRIPT = `
local key = KEYS[1]
local capacity = tonumber(ARGV[1])
local window = tonumber(ARGV[2])
local now = tonumber(ARGV[3])
local state = redis.call('HMGET', key, 'tokens', 'ts')
local tokens = tonumber(state[1])
local ts = tonumber(state[2])
if tokens == nil then tokens = capacity; ts = now end
-- A clock that moved backwards refills nothing.
local refill = math.max(0, now - ts) * capacity / window
tokens = math.min(capacity, tokens + refill)
local allowed = 0
local retry = 0
if tokens >= 1 then
  tokens = tokens - 1
  allowed = 1
else
  retry = math.ceil((1 - tokens) * window / capacity)
end
redis.call('HSET', key, 'tokens', tokens, 'ts', now)
redis.call('PEXPIRE', key, window)
return {allowed, retry}
`;

export type RateResult = { allowed: boolean; retryAfterMs: number };

export async function take(
  redis: Redis,
  key: string,
  capacity: number,
  windowMs: number,
  nowMs: number,
): Promise<RateResult> {
  const [allowed, retry] = (await redis.eval(SCRIPT, 1, key, capacity, windowMs, nowMs)) as [number, number];
  return { allowed: allowed === 1, retryAfterMs: retry };
}

/** Per-user per-feature limits (ARCHITECTURE §14.9). */
export const FEATURE_LIMITS = {
  title: { capacity: 60, windowMs: 3_600_000 },
  rewrite: { capacity: 60, windowMs: 60_000 },
  teammate_draft: { capacity: 10, windowMs: 60_000 },
  stt: { capacity: 30, windowMs: 60_000 },
  auto: { capacity: 30, windowMs: 60_000 },
  voice_session: { capacity: 10, windowMs: 3_600_000 },
  voice_act: { capacity: 60, windowMs: 60_000 },
  voice_resolve: { capacity: 30, windowMs: 60_000 },
} as const;

export async function takeForUser(
  redis: Redis,
  userId: string,
  feature: keyof typeof FEATURE_LIMITS,
  nowMs: number,
): Promise<RateResult> {
  const { capacity, windowMs } = FEATURE_LIMITS[feature];
  return take(redis, `rl:u:${userId}:${feature}`, capacity, windowMs, nowMs);
}
