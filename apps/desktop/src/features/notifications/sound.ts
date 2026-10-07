/** Tandem's own notification sounds (UI-41), synthesized: short, soft and
 * never blocking. "none" is silent. */
const TONES: Record<string, Array<{ freq: number; at: number; dur: number }>> = {
  // A two-note soft bell.
  chime: [
    { freq: 880, at: 0, dur: 0.45 },
    { freq: 1318.5, at: 0.09, dur: 0.5 },
  ],
  soft: [{ freq: 660, at: 0, dur: 0.35 }],
  pop: [{ freq: 1046.5, at: 0, dur: 0.12 }],
};

let ctx: AudioContext | null = null;

export function playSound(name: string, volume = 0.18): void {
  const notes = TONES[name];
  if (!notes) return;
  try {
    ctx ??= new AudioContext();
    const now = ctx.currentTime;
    for (const n of notes) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = n.freq;
      gain.gain.setValueAtTime(0, now + n.at);
      gain.gain.linearRampToValueAtTime(volume, now + n.at + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + n.at + n.dur);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now + n.at);
      osc.stop(now + n.at + n.dur + 0.05);
    }
  } catch {
    // No audio device (CI, a locked-down machine): silence.
  }
}
