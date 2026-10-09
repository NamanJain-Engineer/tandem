import type { Readable } from "node:stream";
import { type SttOptions, type SttProvider, type SttResult, SttUnavailableError } from "./types";

/**
 * Deterministic STT for dev and tests (ARCHITECTURE §14.8): reads the whole
 * stream (so the relay's metering sees all of it), then returns the
 * `x-mock-transcript` header's text or "hello world".
 */
export class MockStt implements SttProvider {
  readonly name = "mock";
  calls: Array<{ bytes: number; language?: string; hints?: string[] }> = [];

  async transcribe(audio: Readable, opts: SttOptions): Promise<SttResult> {
    let bytes = 0;
    for await (const chunk of audio) bytes += (chunk as Buffer).length;
    this.calls.push({ bytes, language: opts.language, hints: opts.hints });
    if (opts.mockFail) throw new SttUnavailableError("mock failure", 503);
    return { text: opts.mockTranscript ?? "hello world", language: opts.language ?? "en" };
  }
}
