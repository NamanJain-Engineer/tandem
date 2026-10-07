/** Injectable time. Tests and `/__test/clock` move it without touching the OS. */
export class Clock {
  private offsetMs = 0;
  private fixed: number | null = null;

  now(): Date {
    return new Date(this.nowMs());
  }

  nowMs(): number {
    return (this.fixed ?? Date.now()) + this.offsetMs;
  }

  nowSec(): number {
    return Math.floor(this.nowMs() / 1000);
  }

  set(at: Date): void {
    this.fixed = null;
    this.offsetMs = at.getTime() - Date.now();
  }

  /** Stops time at the current instant (tests). */
  freeze(): void {
    this.fixed = Date.now();
  }

  advance(ms: number): void {
    this.offsetMs += ms;
  }

  reset(): void {
    this.fixed = null;
    this.offsetMs = 0;
  }
}
