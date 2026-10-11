// Shell-integration marks in the terminal view (F-CD-10, ARCHITECTURE §5.8).
// Rust drops every OSC 133 without this launch's token and strips the token
// from the rest, so each `133;A|B|C|D[;exit]` reaching xterm is the shell's
// own. A prompt (A) is remembered; a command (C … D) gets a ✓ or ✗ beside its
// prompt line; a plain shell is "working" between C and D and "idle" once
// nothing has printed for 2 s after D.

export const IDLE_AFTER_MS = 2_000;

export type Activity = "working" | "idle";

/** What the tracker needs from xterm and the clock (faked in tests). */
export type MarkHost<M> = {
  /** A marker on the cursor's line. */
  marker: () => M | undefined;
  release: (marker: M) => void;
  /** Puts ✓ (`ok`) or ✗ beside the marker's line. */
  decorate: (marker: M, ok: boolean) => void;
  activity: (state: Activity) => void;
  now: () => number;
  setTimer: (fn: () => void, ms: number) => unknown;
  clearTimer: (timer: unknown) => void;
};

export class CommandMarks<M> {
  private prompt: M | undefined;
  /** The current prompt ran a command (its marker carries the ✓/✗). */
  private promptUsed = false;
  private command: M | undefined;
  private running = false;
  private lastOutput = 0;
  private idleTimer: unknown;

  constructor(private host: MarkHost<M>) {}

  /** The OSC 133 payload, e.g. `A`, `C`, `D;0`. */
  mark(data: string): void {
    const [kind, code] = data.split(";");
    if (kind === "A") {
      // A prompt nobody ran a command from is no longer needed.
      if (this.prompt !== undefined && !this.promptUsed) this.host.release(this.prompt);
      this.prompt = this.host.marker();
      this.promptUsed = false;
    } else if (kind === "C") {
      this.command = this.prompt;
      this.promptUsed = true;
      this.running = true;
      this.host.clearTimer(this.idleTimer);
      this.host.activity("working");
    } else if (kind === "D") {
      if (!this.running) return;
      this.running = false;
      const exit = code === undefined || code === "" ? null : Number(code);
      if (this.command !== undefined && exit !== null && Number.isFinite(exit))
        this.host.decorate(this.command, exit === 0);
      this.command = undefined;
      this.waitForIdle();
    }
  }

  /** The open prompt (no command ran from it yet) and the running command's
   * prompt, for parking the view (§5.1): its ✓/✗ marks are the host's. */
  state(): { prompt: M | undefined; running: M | undefined } {
    return {
      prompt: this.promptUsed ? undefined : this.prompt,
      running: this.running ? this.command : undefined,
    };
  }

  /** Output arrived (idle is measured from the last of it). */
  output(): void {
    this.lastOutput = this.host.now();
  }

  dispose(): void {
    this.host.clearTimer(this.idleTimer);
  }

  private waitForIdle(): void {
    this.host.clearTimer(this.idleTimer);
    const check = () => {
      if (this.running) return;
      const quiet = this.host.now() - this.lastOutput;
      if (quiet >= IDLE_AFTER_MS) this.host.activity("idle");
      else this.idleTimer = this.host.setTimer(check, IDLE_AFTER_MS - quiet);
    };
    this.idleTimer = this.host.setTimer(check, IDLE_AFTER_MS);
  }
}
