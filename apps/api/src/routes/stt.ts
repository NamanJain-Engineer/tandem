import { Readable } from "node:stream";
import type { ReadableStream as NodeWebStream } from "node:stream/web";
import busboy from "busboy";
import { Hono } from "hono";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { requirePlan } from "../middleware/requirePlan";
import { MAX_BYTES } from "../services/audio";
import { takeForUser } from "../services/rateLimit";
import { transcribe } from "../services/stt";

/** Room for the multipart framing and the small fields around the audio. */
const ENVELOPE = 64 * 1024;
const LANGUAGE = /^[a-z]{2,3}(-[A-Za-z]{2,4})?$/;

type Upload = {
  fields: Map<string, string[]>;
  file: { stream: Readable; mime: string; filename: string };
};

/**
 * Reads multipart fields up to the `audio` part and hands its stream over
 * without buffering it (busboy keeps nothing on disk). Fields after the audio
 * are ignored, so the desktop sends them first (ARCHITECTURE §14.6).
 */
function receive(contentType: string, body: ReadableStream<Uint8Array>): Promise<Upload> {
  return new Promise((resolve, reject) => {
    let bb: busboy.Busboy;
    try {
      bb = busboy({
        headers: { "content-type": contentType },
        limits: { files: 1, fields: 60, fieldSize: 4096, fileSize: MAX_BYTES + 1 },
      });
    } catch {
      reject(
        new ApiError(
          422,
          "VALIDATION",
          "Unprocessable Entity",
          "Send multipart/form-data with an audio part.",
        ),
      );
      return;
    }
    const fields = new Map<string, string[]>();
    let done = false;
    bb.on("field", (name, value) => {
      fields.set(name, [...(fields.get(name) ?? []), value]);
    });
    bb.on("file", (name, stream, info) => {
      if (name !== "audio" || done) {
        stream.resume();
        return;
      }
      done = true;
      resolve({ fields, file: { stream, mime: info.mimeType, filename: info.filename ?? "" } });
    });
    bb.on("close", () => {
      if (!done)
        reject(new ApiError(422, "VALIDATION", "Unprocessable Entity", "The request has no audio part."));
    });
    bb.on("error", () => {
      if (!done)
        reject(new ApiError(422, "VALIDATION", "Unprocessable Entity", "The upload couldn't be read."));
    });
    Readable.fromWeb(body as unknown as NodeWebStream<Uint8Array>).pipe(bb);
  });
}

/** Cloud speech-to-text (F-BE-07). Desktop bearer, Pro. */
export function sttRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/transcribe", requireUser("bearer"), requirePlan(deps, "cloudStt"), async (c) => {
    const userId = c.get("user")?.id ?? "";
    const length = Number(c.req.header("content-length") ?? "0");
    if (length > MAX_BYTES + ENVELOPE) {
      throw new ApiError(413, "PAYLOAD_TOO_LARGE", "Payload Too Large", "A cloud take is at most 25 MB.", {
        limit: "take_bytes",
      });
    }
    const r = await takeForUser(deps.redis, userId, "stt", deps.clock.nowMs());
    if (!r.allowed) {
      c.header("Retry-After", String(Math.ceil(r.retryAfterMs / 1000)));
      throw new ApiError(429, "RATE_LIMITED", "Too Many Requests", "Slow down and try again shortly.");
    }
    const body = c.req.raw.body;
    const type = c.req.header("content-type") ?? "";
    if (!body || !type.startsWith("multipart/form-data")) {
      throw new ApiError(
        422,
        "VALIDATION",
        "Unprocessable Entity",
        "Send multipart/form-data with an audio part.",
      );
    }
    const { fields, file } = await receive(type, body);
    const one = (k: string) => fields.get(k)?.[0];
    const durationRaw = one("duration_ms");
    const durationMs = durationRaw === undefined ? undefined : Number(durationRaw);
    if (durationMs !== undefined && (!Number.isFinite(durationMs) || durationMs < 0)) {
      file.stream.resume();
      throw new ApiError(
        422,
        "VALIDATION",
        "Unprocessable Entity",
        "duration_ms must be a number of milliseconds.",
      );
    }
    const language = one("language");
    if (language && language !== "auto" && !LANGUAGE.test(language)) {
      file.stream.resume();
      throw new ApiError(
        422,
        "VALIDATION",
        "Unprocessable Entity",
        "language must be a language code or auto.",
      );
    }
    const hints = (fields.get("hints") ?? []).map((h) => h.trim().slice(0, 100)).filter(Boolean);
    if (hints.length > 50) {
      file.stream.resume();
      throw new ApiError(422, "VALIDATION", "Unprocessable Entity", "Send at most 50 hints.");
    }
    const mock = deps.stt.name === "mock";
    return c.json(
      await transcribe(deps, userId, {
        audio: file.stream,
        mime: file.mime,
        filename: file.filename,
        durationMs,
        language: language && language !== "auto" ? language : undefined,
        hints: hints.length ? hints : undefined,
        mockTranscript: mock ? c.req.header("x-mock-transcript") : undefined,
        mockFail: mock && c.req.header("x-mock-stt-fail") === "1",
      }),
    );
  });

  return app;
}
