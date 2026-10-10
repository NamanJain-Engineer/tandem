import type { Readable } from "node:stream";

export type SttOptions = {
  mime: string;
  filename: string;
  /** An ISO 639-1 code, or undefined to auto-detect. */
  language?: string;
  /** Preferred words, sent only when the user turned on dictionary hints. */
  hints?: string[];
  /** Dev and tests only: what MockStt returns (`x-mock-transcript`). */
  mockTranscript?: string;
  /** Dev and tests only: MockStt fails the call (`x-mock-stt-fail`). */
  mockFail?: boolean;
};

export type SttResult = { text: string; language: string | null };

/**
 * Speech-to-text for the relay (F-BE-07). Implementations read the audio as a
 * stream and never write it anywhere, and never log it or the text.
 */
export interface SttProvider {
  readonly name: string;
  transcribe(audio: Readable, opts: SttOptions): Promise<SttResult>;
}

export class SttUnavailableError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
  ) {
    super(message);
    this.name = "SttUnavailableError";
  }
}
