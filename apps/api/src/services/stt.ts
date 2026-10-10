// The STT relay (F-BE-07, ARCHITECTURE §14.8): daily cap, reserve → stream
// to the provider while metering → settle per started 15 s, or refund. The
// audio is never buffered whole, written to disk or logged.
import { pipeline } from "node:stream/promises";
import type { Readable } from "node:stream";
import { and, eq, sql } from "drizzle-orm";
import type { Deps } from "../deps";
import { aiRequests, dailyUsage } from "../db/schema";
import { newId } from "../lib/ids";
import { logger } from "../lib/logger";
import { ApiError } from "../lib/problem";
import { SttUnavailableError } from "../providers/stt/types";
import { AudioLimitError, AudioMeter, kindOf, sttCost } from "./audio";
import { notifyCredits } from "./credits";
import { refund, reserve, settle } from "./ledger";

export const MAX_TAKE_SECONDS = 600;

export type SttRequest = {
  audio: Readable;
  mime: string;
  filename: string;
  /** What the desktop says the take lasts; the relay holds it to that. */
  durationMs?: number;
  language?: string;
  hints?: string[];
  mockTranscript?: string;
  mockFail?: boolean;
};

export type SttResponse = { text: string; language: string | null; duration: number };

const day = (deps: Deps) => deps.clock.now().toISOString().slice(0, 10);

async function usedToday(deps: Deps, userId: string): Promise<number> {
  const [row] = await deps.db
    .select({ units: dailyUsage.units })
    .from(dailyUsage)
    .where(and(eq(dailyUsage.userId, userId), eq(dailyUsage.feature, "stt"), eq(dailyUsage.day, day(deps))))
    .limit(1);
  return row?.units ?? 0;
}

/** Holds `seconds` of today's cap; false when that would pass it. */
async function holdSeconds(deps: Deps, userId: string, seconds: number, cap: number): Promise<boolean> {
  const rows = await deps.db
    .insert(dailyUsage)
    .values({ userId, feature: "stt", day: day(deps), units: seconds })
    .onConflictDoUpdate({
      target: [dailyUsage.userId, dailyUsage.feature, dailyUsage.day],
      set: { units: sql`${dailyUsage.units} + ${seconds}` },
      setWhere: sql`${dailyUsage.units} + ${seconds} <= ${cap}`,
    })
    .returning({ units: dailyUsage.units });
  return rows.length > 0 && (rows[0]?.units ?? 0) <= cap;
}

async function releaseSeconds(deps: Deps, userId: string, seconds: number): Promise<void> {
  if (seconds <= 0) return;
  await deps.db
    .update(dailyUsage)
    .set({ units: sql`greatest(${dailyUsage.units} - ${seconds}, 0)` })
    .where(and(eq(dailyUsage.userId, userId), eq(dailyUsage.feature, "stt"), eq(dailyUsage.day, day(deps))));
}

const tooLong = () =>
  new ApiError(413, "PAYLOAD_TOO_LARGE", "Payload Too Large", "A cloud take is at most 10 minutes.", {
    limit: "take_minutes",
  });
const tooBig = () =>
  new ApiError(413, "PAYLOAD_TOO_LARGE", "Payload Too Large", "A cloud take is at most 25 MB.", {
    limit: "take_bytes",
  });
const dailyLimit = (minutes: number) =>
  new ApiError(
    429,
    "DAILY_LIMIT",
    "Too Many Requests",
    `You've reached today's cloud limit of ${minutes} minutes. Switch to Local.`,
    { limit: "stt_minutes" },
  );

export async function transcribe(deps: Deps, userId: string, req: SttRequest): Promise<SttResponse> {
  const kind = kindOf(req.mime, req.filename);
  if (!kind) {
    req.audio.resume();
    throw new ApiError(415, "UNSUPPORTED_MEDIA", "Unsupported Media Type", "Send Opus (Ogg) or WAV audio.");
  }
  const capMinutes = deps.config.config.daily_caps.stt_minutes ?? 120;
  const capSeconds = capMinutes * 60;
  const declared = req.durationMs !== undefined ? req.durationMs / 1000 : undefined;
  if (declared !== undefined && declared > MAX_TAKE_SECONDS) {
    req.audio.resume();
    throw tooLong();
  }
  const remaining = capSeconds - (await usedToday(deps, userId));
  const wanted = declared !== undefined ? Math.ceil(declared) + 1 : MAX_TAKE_SECONDS;
  // Without a declared length, a take may use what's left of today's cap.
  const hold = declared !== undefined ? Math.min(wanted, MAX_TAKE_SECONDS) : Math.min(wanted, remaining);
  if (remaining <= 0 || (declared !== undefined && Math.ceil(declared) > remaining)) {
    req.audio.resume();
    throw dailyLimit(capMinutes);
  }
  if (!(await holdSeconds(deps, userId, Math.min(hold, remaining), capSeconds))) {
    req.audio.resume();
    throw dailyLimit(capMinutes);
  }
  const held = Math.min(hold, remaining);
  const perBlock = deps.config.config.prices.stt_15s ?? 2;
  let reservation: string;
  try {
    reservation = await reserve(deps.db, {
      userId,
      feature: "stt",
      amount: sttCost(held, perBlock),
      now: deps.clock.now(),
    });
  } catch (err) {
    await releaseSeconds(deps, userId, held);
    req.audio.resume();
    throw err;
  }

  const meter = new AudioMeter(kind, held);
  const started = Date.now();
  let meterError: unknown = null;
  const flowing = pipeline(req.audio, meter).catch((err: unknown) => {
    meterError = err;
  });
  let result;
  let failure: ApiError | null = null;
  try {
    result = await deps.stt.transcribe(meter, {
      mime: req.mime,
      filename: req.filename,
      language: req.language,
      hints: req.hints,
      mockTranscript: req.mockTranscript,
      mockFail: req.mockFail,
    });
    await flowing;
  } catch (err) {
    await flowing;
    // The provider's own errors carry a status only; anything else is named, never shown.
    if (!(err instanceof SttUnavailableError) && !(err instanceof AudioLimitError)) {
      logger.error({ err: err instanceof Error ? err.name : "unknown" }, "stt relay failed");
    }
  }
  if (meterError instanceof AudioLimitError) {
    const reason = meterError.reason;
    failure =
      reason === "bytes"
        ? tooBig()
        : reason === "malformed"
          ? new ApiError(422, "VALIDATION", "Unprocessable Entity", "The audio couldn't be read.")
          : declared !== undefined
            ? new ApiError(
                422,
                "VALIDATION",
                "Unprocessable Entity",
                "The audio is longer than duration_ms says.",
              )
            : held < MAX_TAKE_SECONDS
              ? dailyLimit(capMinutes)
              : tooLong();
  } else if (!result) {
    failure = new ApiError(
      503,
      "STT_UNAVAILABLE",
      "Service Unavailable",
      "Cloud transcription didn't answer. Try again.",
      {
        retryable: true,
      },
    );
  }
  const seconds = meter.seconds;
  const latencyMs = Date.now() - started;
  if (failure || !result) {
    req.audio.destroy();
    const back = await refund(deps.db, reservation);
    if (back) await notifyCredits(deps, back.owner);
    await releaseSeconds(deps, userId, held);
    await recordStt(deps, userId, failure?.status ?? 503, 0, latencyMs, seconds);
    throw failure ?? new ApiError(503, "STT_UNAVAILABLE", "Service Unavailable");
  }
  const cost = sttCost(seconds, perBlock);
  const settled = await settle(deps.db, reservation, cost);
  if (settled) await notifyCredits(deps, settled.owner);
  await releaseSeconds(deps, userId, held - Math.ceil(seconds));
  await recordStt(deps, userId, 200, cost, latencyMs, seconds);
  return { text: result.text, language: result.language, duration: Math.round(seconds * 100) / 100 };
}

/** Metadata only: no audio, no text (ARCHITECTURE §14.8). */
async function recordStt(
  deps: Deps,
  userId: string,
  status: number,
  credits: number,
  latencyMs: number,
  seconds: number,
): Promise<void> {
  await deps.db.insert(aiRequests).values({
    id: newId("air"),
    userId,
    feature: "stt",
    model: deps.stt.name,
    status,
    credits,
    latencyMs,
    tokensIn: Math.round(seconds),
  });
  logger.info(
    { feature: "stt", provider: deps.stt.name, status, latencyMs, seconds: Math.round(seconds), credits },
    "stt request",
  );
}
