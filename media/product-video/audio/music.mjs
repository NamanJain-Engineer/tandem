// Synthesizes the soundtrack and sound effects for the Vibezeo product video.
// Everything is generated here (no samples), so we own all of it.
// 120 BPM: one beat = 0.5 s, one bar = 2 s. Scene cuts sit on bar lines.
//
//   node audio/music.mjs out.wav
import { writeFileSync } from "node:fs";

const SR = 48000;
const DUR = 64.5;
const N = Math.ceil(SR * DUR);
const BEAT = 0.5;
const BAR = 2;

// ---------- helpers ----------
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = rng(20261007);
const noise = () => rand() * 2 - 1;
const hz = (m) => 440 * Math.pow(2, (m - 69) / 12);
const clamp = (x, a, b) => Math.max(a, Math.min(b, x));
const bus = () => [new Float32Array(N), new Float32Array(N)];
const panLR = (p) => [Math.cos(((p + 1) * Math.PI) / 4), Math.sin(((p + 1) * Math.PI) / 4)];

class SVF {
  constructor() {
    this.a = 0;
    this.b = 0;
    this.lp = 0;
    this.bp = 0;
    this.hp = 0;
    this.set(1000, 0.707);
  }
  set(fc, q) {
    const g = Math.tan((Math.PI * clamp(fc, 20, SR * 0.45)) / SR);
    this.k = 1 / q;
    this.a1 = 1 / (1 + g * (g + this.k));
    this.a2 = g * this.a1;
    this.a3 = g * this.a2;
  }
  run(x) {
    const v3 = x - this.b;
    const v1 = this.a1 * this.a + this.a2 * v3;
    const v2 = this.b + this.a2 * this.a + this.a3 * v3;
    this.a = 2 * v1 - this.a;
    this.b = 2 * v2 - this.b;
    this.lp = v2;
    this.bp = v1;
    this.hp = x - this.k * v1 - v2;
    return v2;
  }
}

function blep(t, dt) {
  if (t < dt) {
    t /= dt;
    return t + t - t * t - 1;
  }
  if (t > 1 - dt) {
    t = (t - 1) / dt;
    return t * t + t + t + 1;
  }
  return 0;
}

function adsr(t, dur, a, d, s, r) {
  if (t < 0) return 0;
  let v;
  if (t < a) v = t / a;
  else if (t < a + d) v = 1 - ((t - a) / d) * (1 - s);
  else v = s;
  if (t > dur) v *= Math.max(0, 1 - (t - dur) / r);
  return v;
}

// ---------- buses ----------
const pad = bus();
const arp = bus();
const bass = bus();
const drums = bus();
const sfx = bus();
const revSend = bus();
const delaySend = bus();

function add(b, i, l, r) {
  if (i >= 0 && i < N) {
    b[0][i] += l;
    b[1][i] += r;
  }
}

// ---------- arrangement ----------
// vi–IV–I–V in C, resolving to C for the end card.
const CHORDS = {
  Am: { root: 45, pad: [57, 60, 64, 67], arp: [69, 72, 76, 79] },
  F: { root: 41, pad: [57, 60, 65, 67], arp: [65, 69, 72, 77] },
  C: { root: 48, pad: [55, 60, 64, 67], arp: [67, 72, 76, 79] },
  G: { root: 43, pad: [55, 59, 62, 67], arp: [67, 71, 74, 79] },
};
const PROG = ["Am", "F", "C", "G"];
const chordAt = (bar) => (bar >= 28 ? "C" : PROG[bar % 4]);

// Section helpers (seconds).
const full = (t) => (t >= 8 && t < 28) || (t >= 36 && t < 56);
const breakdown = (t) => t >= 28 && t < 35.2;
const outro = (t) => t >= 56;

// Pad: three detuned saws per note through a lowpass that opens over the intro.
function renderPad() {
  for (let bar = 0; bar < 32; bar++) {
    const chord = CHORDS[chordAt(bar)];
    const t0 = bar * BAR;
    const len = bar === 31 ? 3.5 : BAR;
    const outroHold = bar >= 28;
    if (outroHold && bar > 28) continue; // one long chord from 56 s
    const dur = outroHold ? 6.0 : len;
    chord.pad.forEach((m, ni) => {
      for (const det of [-9, 0, 9]) {
        const f = hz(m) * Math.pow(2, det / 1200);
        const [pl, pr] = panLR(det / 12 + (ni - 1.5) * 0.12);
        const filt = new SVF();
        let ph = rand();
        const start = Math.floor(t0 * SR);
        const end = Math.min(N, Math.floor((t0 + dur + 1.2) * SR));
        for (let i = start; i < end; i++) {
          const t = i / SR;
          const lt = t - t0;
          if ((i - start) % 32 === 0) {
            let fc = 1900;
            if (t < 4) fc = 700 + (t / 4) * 1200;
            else if (breakdown(t)) fc = 1300 + 600 * Math.sin((t - 28) * 0.6);
            else if (outro(t)) fc = 2200 - Math.max(0, t - 58) * 220;
            filt.set(fc, 0.8);
          }
          const dt = f / SR;
          ph += dt;
          if (ph >= 1) ph -= 1;
          const s = 2 * ph - 1 - blep(ph, dt);
          const e = adsr(lt, dur, 0.35, 0.6, 0.85, 1.0);
          const v = filt.run(s) * e * (t < 4 ? 0.034 : 0.028);
          add(pad, i, v * pl, v * pr);
        }
      }
    });
  }
}

// Pluck arpeggio on 8ths with a ping-pong delay send.
function renderArp() {
  const pattern = [0, 1, 2, 3, 2, 1, 2, 3];
  for (let step = 0; step < 128; step++) {
    const t0 = step * (BEAT / 2);
    if (t0 >= 61) break;
    if (t0 >= 35.2 && t0 < 36) continue;
    const bar = Math.floor(t0 / BAR);
    const chord = CHORDS[chordAt(bar)];
    let m = chord.arp[pattern[step % 8]];
    if (outro(t0) && step % 2 === 1) continue; // slower at the end
    let amp = 0.13;
    if (t0 < 4) amp = 0.08 + 0.04 * (t0 / 4);
    else if (breakdown(t0)) amp = 0.09;
    else if (outro(t0)) amp = 0.08 * Math.max(0, 1 - (t0 - 56) / 6);
    const accent = step % 4 === 0 ? 1.15 : 1;
    const f = hz(m);
    const filt = new SVF();
    const start = Math.floor(t0 * SR);
    const len = Math.floor(0.42 * SR);
    let ph = 0;
    let ph2 = 0.5;
    const pan = Math.sin(step * 1.7) * 0.35;
    const [pl, pr] = panLR(pan);
    for (let j = 0; j < len; j++) {
      const lt = j / SR;
      if (j % 16 === 0) filt.set(500 + 4200 * Math.exp(-lt / 0.07), 1.1);
      const dt = f / SR;
      ph += dt;
      if (ph >= 1) ph -= 1;
      ph2 += dt * 1.004;
      if (ph2 >= 1) ph2 -= 1;
      const s = (2 * ph - 1 - blep(ph, dt) + (2 * ph2 - 1 - blep(ph2, dt * 1.004))) * 0.5;
      const e = Math.min(1, lt / 0.003) * Math.exp(-lt / 0.16);
      const v = filt.run(s) * e * amp * accent;
      add(arp, start + j, v * pl, v * pr);
      add(delaySend, start + j, v * 0.5, v * 0.5);
    }
  }
}

// Bass: sub sine plus a filtered saw on 8ths during the full sections.
function renderBass() {
  for (let step = 0; step < 128; step++) {
    const t0 = step * (BEAT / 2);
    const playing = full(t0) || breakdown(t0) || (t0 >= 56 && t0 < 58);
    if (!playing) continue;
    const bar = Math.floor(t0 / BAR);
    const root = CHORDS[chordAt(bar)].root;
    const oct = step % 2 === 1 ? 12 : 0;
    const f = hz(root + (breakdown(t0) ? 0 : oct));
    const gate = breakdown(t0) ? 0.24 : 0.2;
    if (breakdown(t0) && step % 2 === 1) continue;
    const filt = new SVF();
    filt.set(520, 0.9);
    const start = Math.floor(t0 * SR);
    const len = Math.floor((gate + 0.08) * SR);
    let ph = 0;
    let sp = 0;
    for (let j = 0; j < len; j++) {
      const lt = j / SR;
      const dt = f / SR;
      ph += dt;
      if (ph >= 1) ph -= 1;
      sp += (2 * Math.PI * f) / SR;
      const saw = 2 * ph - 1 - blep(ph, dt);
      const e = adsr(lt, gate, 0.004, 0.08, 0.7, 0.06);
      const v = (Math.sin(sp) * 0.75 + filt.run(saw) * 0.45) * e * 0.2;
      add(bass, start + j, v, v);
    }
  }
}

// ---------- drums ----------
const kicks = [];
function kick(t0, amp = 1) {
  kicks.push(t0);
  const start = Math.floor(t0 * SR);
  const len = Math.floor(0.45 * SR);
  let ph = 0;
  for (let j = 0; j < len; j++) {
    const lt = j / SR;
    const f = 46 + 110 * Math.exp(-lt / 0.035);
    ph += (2 * Math.PI * f) / SR;
    const body = Math.sin(ph) * Math.exp(-lt / 0.26);
    const click = noise() * Math.exp(-lt / 0.002) * 0.3;
    const v = Math.tanh((body + click) * 1.6) * 0.31 * amp;
    add(drums, start + j, v, v);
  }
}
function clap(t0, amp = 1) {
  const start = Math.floor(t0 * SR);
  const len = Math.floor(0.35 * SR);
  const f = new SVF();
  f.set(1400, 1.2);
  for (let j = 0; j < len; j++) {
    const lt = j / SR;
    let e = Math.exp(-lt / 0.09);
    for (const o of [0, 0.011, 0.022]) if (lt >= o && lt < o + 0.008) e = Math.max(e, 1 - (lt - o) / 0.008);
    f.run(noise());
    const v = f.bp * e * 0.4 * amp;
    add(drums, start + j, v * 0.95, v);
    add(revSend, start + j, v * 0.6, v * 0.6);
  }
}
function hat(t0, amp = 1, open = false) {
  const start = Math.floor(t0 * SR);
  const dec = open ? 0.16 : 0.035;
  const len = Math.floor(dec * 5 * SR);
  const f = new SVF();
  f.set(8500, 0.9);
  const pan = open ? -0.2 : 0.25;
  const [pl, pr] = panLR(pan);
  for (let j = 0; j < len; j++) {
    const lt = j / SR;
    f.run(noise());
    const v = f.hp * Math.exp(-lt / dec) * 0.11 * amp;
    add(drums, start + j, v * pl, v * pr);
  }
}
function renderDrums() {
  for (let b = 0; b < 128; b++) {
    const t = b * BEAT;
    if (full(t)) {
      kick(t);
      if (b % 2 === 1) clap(t, 0.85);
      hat(t + BEAT / 2, 0.9, b % 4 === 3 && t >= 36);
      hat(t + BEAT / 4, 0.35);
      hat(t + (3 * BEAT) / 4, 0.35);
    } else if (breakdown(t)) {
      hat(t + BEAT / 2, 0.45);
    } else if (t >= 4 && t < 8 && b % 4 === 0) {
      kick(t, 0.6);
    }
  }
  // Snare roll into the end card.
  for (let k = 0; k < 16; k++) {
    const t = 54 + k * 0.125;
    clap(t, 0.25 + (k / 16) * 0.6);
  }
  kick(56, 1.1);
}

// ---------- sound effects ----------
function riser(t0, t1, amp = 1) {
  const start = Math.floor(t0 * SR);
  const len = Math.floor((t1 - t0) * SR);
  const f = new SVF();
  let ph = 0;
  for (let j = 0; j < len; j++) {
    const p = j / len;
    if (j % 32 === 0) f.set(300 + 7000 * p * p, 2.2);
    f.run(noise());
    ph += (2 * Math.PI * (200 + 900 * p * p)) / SR;
    const e = p * p * amp;
    const v = (f.bp * 0.5 + Math.sin(ph) * 0.05) * e * 0.5;
    add(sfx, start + j, v, v);
    add(revSend, start + j, v * 0.4, v * 0.4);
  }
}
function impact(t0, amp = 1) {
  const start = Math.floor(t0 * SR);
  const len = Math.floor(2.6 * SR);
  let ph = 0;
  const f = new SVF();
  f.set(2500, 0.7);
  for (let j = 0; j < len; j++) {
    const lt = j / SR;
    ph += (2 * Math.PI * (32 + 45 * Math.exp(-lt / 0.25))) / SR;
    f.run(noise());
    const v = (Math.sin(ph) * Math.exp(-lt / 0.9) * 0.55 + f.lp * Math.exp(-lt / 0.18) * 0.35) * amp;
    add(sfx, start + j, v, v);
    add(revSend, start + j, v * 0.5, v * 0.5);
  }
}
function whoosh(t0, dur = 0.7, amp = 1, pan = 0) {
  const start = Math.floor((t0 - dur * 0.6) * SR);
  const len = Math.floor(dur * SR);
  const f = new SVF();
  for (let j = 0; j < len; j++) {
    const p = j / len;
    if (j % 32 === 0) f.set(400 + 5000 * Math.sin(Math.PI * p), 1.6);
    f.run(noise());
    const e = Math.sin(Math.PI * p) ** 2 * amp;
    const sweep = pan + (p - 0.5) * 1.2;
    const [pl, pr] = panLR(clamp(sweep, -1, 1));
    const v = f.bp * e * 0.32;
    add(sfx, start + j, v * pl, v * pr);
    add(revSend, start + j, v * 0.3, v * 0.3);
  }
}
function tick(t0, amp = 1, freq = 2200) {
  const start = Math.floor(t0 * SR);
  const len = Math.floor(0.05 * SR);
  for (let j = 0; j < len; j++) {
    const lt = j / SR;
    const v = Math.sin(2 * Math.PI * freq * lt) * Math.exp(-lt / 0.008) * 0.18 * amp;
    add(sfx, start + j, v, v);
    add(revSend, start + j, v * 0.25, v * 0.25);
  }
}
function pop(t0, amp = 1, freq = 660) {
  const start = Math.floor(t0 * SR);
  const len = Math.floor(0.16 * SR);
  let ph = 0;
  for (let j = 0; j < len; j++) {
    const lt = j / SR;
    ph += (2 * Math.PI * freq * (1 + 0.6 * Math.exp(-lt / 0.02))) / SR;
    const v = Math.sin(ph) * Math.exp(-lt / 0.045) * 0.16 * amp;
    add(sfx, start + j, v, v);
    add(revSend, start + j, v * 0.35, v * 0.35);
  }
}
function key(t0, amp = 1) {
  const start = Math.floor(t0 * SR);
  const len = Math.floor(0.06 * SR);
  const f = new SVF();
  f.set(3200 + rand() * 1500, 2.5);
  for (let j = 0; j < len; j++) {
    const lt = j / SR;
    f.run(noise());
    const thock = Math.sin(2 * Math.PI * 180 * lt) * Math.exp(-lt / 0.01) * 0.5;
    const v = (f.bp * Math.exp(-lt / 0.006) + thock) * 0.25 * amp;
    add(sfx, start + j, v, v);
  }
}
function glitch(t0, dur) {
  const start = Math.floor(t0 * SR);
  const len = Math.floor(dur * SR);
  let hold = 0;
  let val = 0;
  let gate = 1;
  for (let j = 0; j < len; j++) {
    if (j % 600 === 0) gate = rand() > 0.35 ? 1 : 0;
    if (hold-- <= 0) {
      hold = 8 + Math.floor(rand() * 60);
      val = noise();
    }
    const tone = Math.sign(Math.sin((2 * Math.PI * (j % 4800 < 2400 ? 880 : 1320) * j) / SR));
    const v = (val * 0.5 + tone * 0.25) * gate * 0.22;
    add(sfx, start + j, v, v * 0.9);
  }
}

function renderSfx() {
  // Hook: engine names land on each beat.
  [0, 0.5, 1.0, 1.5].forEach((t, i) => pop(t, 1.6, 520 + i * 90));
  [2.0, 2.25, 2.5, 2.75, 3.0, 3.25].forEach((t, i) => tick(t, 0.6 + i * 0.08, 1800 + i * 150));
  riser(2.0, 4.0, 0.9);
  whoosh(4.0, 0.9, 0.8);
  impact(4.0, 1);
  pop(5.0, 0.6, 780);
  // Scene changes.
  for (const t of [8, 14, 28, 46, 52]) whoosh(t, 0.7, 0.85, t % 4 === 0 ? -0.3 : 0.3);
  // Callouts and cards.
  [9.0, 10.2, 14.73, 15.7, 16.2, 17.33, 20.0, 22.73, 25.73, 29.0, 29.6, 30.2, 30.8, 40.17, 43.17, 47.0, 47.6, 48.2, 53.0, 53.2, 53.4].forEach((t, i) =>
    pop(t, 0.55, 620 + (i % 4) * 110),
  );
  // Status pills changing state in the live-status scene.
  [31.5, 32.5, 33.25].forEach((t) => tick(t, 0.7, 2600));
  // "kill -9" typed, then the crash.
  for (let i = 1; i <= 15; i++) if (i !== 5 && i !== 8) key(34 + i * 0.055, 0.8 + 0.2 * rand());
  // Keycaps pressed in the terminals scene.
  for (const t of [15.7, 20.0]) [0, 0.1, 0.2].forEach((d) => key(t + 0.27 + d, 0.7));
  key(35.0, 1.2);
  riser(32.0, 35.2, 0.7);
  glitch(35.75, 0.25);
  impact(36.0, 0.8);
  whoosh(56.0, 1.0, 0.9);
  impact(56.0, 1.1);
  pop(57.0, 0.5, 880);
}

// ---------- effects ----------
// Freeverb-style reverb (8 combs + 4 allpasses per channel).
function reverb([inL, inR], room = 0.84, damp = 0.3, wet = 0.32) {
  const combs = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617].map((x) => Math.round((x * SR) / 44100));
  const alls = [556, 441, 341, 225].map((x) => Math.round((x * SR) / 44100));
  const out = bus();
  [inL, inR].forEach((input, ch) => {
    const spread = ch === 0 ? 0 : 23;
    const cbufs = combs.map((c) => ({ b: new Float32Array(c + spread), i: 0, f: 0 }));
    const abufs = alls.map((a) => ({ b: new Float32Array(a + spread), i: 0 }));
    const o = out[ch];
    for (let n = 0; n < N; n++) {
      const x = input[n] * 0.015;
      let s = 0;
      for (const c of cbufs) {
        const y = c.b[c.i];
        c.f = y * (1 - damp) + c.f * damp;
        c.b[c.i] = x + c.f * room;
        if (++c.i >= c.b.length) c.i = 0;
        s += y;
      }
      for (const a of abufs) {
        const bv = a.b[a.i];
        const y = -s + bv;
        a.b[a.i] = s + bv * 0.5;
        if (++a.i >= a.b.length) a.i = 0;
        s = y;
      }
      o[n] = s * wet * 3;
    }
  });
  return out;
}
function pingPong([inL, inR], time = 0.375, fb = 0.38, mix = 0.35) {
  const d = Math.floor(time * SR);
  const out = bus();
  const bl = new Float32Array(d);
  const br = new Float32Array(d);
  let i = 0;
  const lpL = new SVF();
  const lpR = new SVF();
  lpL.set(3500, 0.7);
  lpR.set(3500, 0.7);
  for (let n = 0; n < N; n++) {
    const yl = bl[i];
    const yr = br[i];
    bl[i] = lpL.run(inL[n] + inR[n] + yr * fb);
    br[i] = lpR.run(yl * fb);
    if (++i >= d) i = 0;
    out[0][n] = yl * mix;
    out[1][n] = yr * mix;
  }
  return out;
}

// ---------- mix ----------
renderPad();
renderArp();
renderBass();
renderDrums();
renderSfx();
// Pad and arp feed the reverb too.
for (let n = 0; n < N; n++) {
  revSend[0][n] += pad[0][n] * 0.5 + arp[0][n] * 0.35;
  revSend[1][n] += pad[1][n] * 0.5 + arp[1][n] * 0.35;
}
const rev = reverb(revSend);
const dly = pingPong(delaySend);

// Sidechain: pad, bass and arp duck under each kick.
const duck = new Float32Array(N).fill(1);
for (const k of kicks) {
  const s = Math.floor(k * SR);
  for (let j = 0; j < Math.floor(0.4 * SR) && s + j < N; j++) {
    const lt = j / SR;
    const d = 1 - 0.55 * Math.exp(-lt / 0.11) * Math.min(1, lt / 0.004 + 0.4);
    duck[s + j] = Math.min(duck[s + j], d);
  }
}

const mus = bus();
for (let n = 0; n < N; n++) {
  const d = duck[n];
  for (let c = 0; c < 2; c++) {
    mus[c][n] = (pad[c][n] + arp[c][n] + bass[c][n] + dly[c][n]) * d + drums[c][n] + rev[c][n] * 0.8;
  }
}

// Tape stop on the music before the crash (35.2–35.75 s), then silence until the drop.
{
  const a = Math.floor(35.2 * SR);
  const b = Math.floor(35.75 * SR);
  const copy = [mus[0].slice(a, b), mus[1].slice(a, b)];
  let pos = 0;
  for (let n = a; n < b; n++) {
    const p = (n - a) / (b - a);
    const rate = Math.max(0, 1 - p) ** 1.5;
    pos += rate;
    const i0 = Math.floor(pos);
    const fr = pos - i0;
    for (let c = 0; c < 2; c++) {
      const v = (copy[c][i0] ?? 0) * (1 - fr) + (copy[c][i0 + 1] ?? 0) * fr;
      mus[c][n] = v * (1 - p * 0.6);
    }
  }
  const z = Math.floor(36.0 * SR);
  for (let n = b; n < z; n++) {
    mus[0][n] *= 0.0;
    mus[1][n] *= 0.0;
  }
}

const out = bus();
for (let n = 0; n < N; n++) {
  const t = n / SR;
  const fadeIn = Math.min(1, t / 0.05);
  const fadeOut = t > 61 ? Math.max(0, 1 - (t - 61) / 3.4) : 1;
  for (let c = 0; c < 2; c++) {
    const x = (mus[c][n] + sfx[c][n]) * fadeIn * fadeOut * 1.4;
    out[c][n] = Math.tanh(x * 1.2) / 1.2;
  }
}
let peak = 0;
for (let n = 0; n < N; n++) peak = Math.max(peak, Math.abs(out[0][n]), Math.abs(out[1][n]));
const gain = 0.89 / peak;

// 16-bit stereo WAV.
const buf = Buffer.alloc(44 + N * 4);
buf.write("RIFF", 0);
buf.writeUInt32LE(36 + N * 4, 4);
buf.write("WAVEfmt ", 8);
buf.writeUInt32LE(16, 16);
buf.writeUInt16LE(1, 20);
buf.writeUInt16LE(2, 22);
buf.writeUInt32LE(SR, 24);
buf.writeUInt32LE(SR * 4, 28);
buf.writeUInt16LE(4, 32);
buf.writeUInt16LE(16, 34);
buf.write("data", 36);
buf.writeUInt32LE(N * 4, 40);
for (let n = 0; n < N; n++) {
  buf.writeInt16LE(Math.round(clamp(out[0][n] * gain, -1, 1) * 32767), 44 + n * 4);
  buf.writeInt16LE(Math.round(clamp(out[1][n] * gain, -1, 1) * 32767), 46 + n * 4);
}
writeFileSync(process.argv[2] ?? "soundtrack.wav", buf);
console.log(`wrote ${DUR}s, peak ${peak.toFixed(3)}`);
