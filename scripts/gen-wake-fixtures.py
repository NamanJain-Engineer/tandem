#!/usr/bin/env python3
"""Generates the wake-phrase test clips (F-CN-07 AC1) in
crates/tandem-voice/tests/fixtures/wake/.

The clips are synthetic: Piper's LibriTTS-R voice (904 speakers, CC BY 4.0)
through sherpa-onnx's TTS. They stand in for recordings of real people until
the manual benchmark on real hardware is done (HUMAN_TODO).

  python3 -m venv /tmp/v && /tmp/v/bin/pip install sherpa-onnx numpy
  curl -LO https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-piper-en_US-libritts_r-medium.tar.bz2
  tar xjf vits-piper-en_US-libritts_r-medium.tar.bz2
  /tmp/v/bin/python -I scripts/gen-wake-fixtures.py vits-piper-en_US-libritts_r-medium crates/tandem-voice/tests/fixtures/wake

Speakers 0..899 were split: the thresholds were tuned on other speakers, and
the true clips use speakers chosen with seed 2026 from the rest (held out).
"""
import sys, wave
import numpy as np
import sherpa_onnx as so

tts_dir, out = sys.argv[1], sys.argv[2]
tts = so.OfflineTts(so.OfflineTtsConfig(model=so.OfflineTtsModelConfig(
    vits=so.OfflineTtsVitsModelConfig(model=f"{tts_dir}/en_US-libritts_r-medium.onnx",
        tokens=f"{tts_dir}/tokens.txt", data_dir=f"{tts_dir}/espeak-ng-data"), num_threads=4)))

def say(text, sid, speed):
    a = tts.generate(text, sid=sid, speed=speed)
    s = np.array(a.samples, dtype=np.float32)
    n = int(len(s) * 16000 / a.sample_rate)
    return np.interp(np.linspace(0, len(s) - 1, n), np.arange(len(s)), s).astype(np.float32)

def write(path, x, width):
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(width); w.setframerate(16000)
        x = np.clip(x, -1, 1)
        if width == 2:
            w.writeframes((x * 32767).astype("<i2").tobytes())
        else:  # 8-bit WAV is unsigned
            w.writeframes((x * 127 + 128).astype(np.uint8).tobytes())

tuned = set(np.random.default_rng(5).choice(900, 30, replace=False).tolist())
rng = np.random.default_rng(2026)
held_out = [s for s in rng.permutation(900).tolist() if s not in tuned][:10]
for i, sid in enumerate(held_out):
    x = say("Hey Conductor", sid, float(rng.uniform(0.9, 1.1)))
    x = np.concatenate([np.zeros(4000, np.float32), x, np.zeros(8000, np.float32)])
    x = x + rng.normal(0, 0.003, len(x)).astype(np.float32)
    write(f"{out}/hey_conductor_{i:02}.wav", x, 2)

GENERAL = [
"Can you open the billing pane and run the tests again", "I think the auth tests are failing on main",
"Okay let me read the error message first", "We should refactor the parser before Friday",
"Please summarize what the agent did in the docs pane", "Can you hear me now, the microphone was muted",
"Running the migration on the staging database now", "The deadline moved to next week, so relax",
"Make sure the worktree is clean before merging", "We need more coffee in the kitchen",
"Open settings and change the theme to dark", "That was a great conference talk",
"The contractor is coming on Monday", "I'll ask the driver when we arrive",
"Let me share my screen so you can see the logs", "The build takes about four minutes on my laptop",
"Did anyone update the changelog for this release", "I'm going to grab lunch, back in twenty minutes",
"The weather looks great for the weekend", "Turn the volume down a little please",
"My cat keeps walking across the keyboard", "Ship it once the reviewer approves",
"Which branch has the fix for the login bug", "The meeting starts at half past two",
"Let's pair on the flaky test this afternoon", "Restart the server and clear the cache",
"I prefer tabs, but the formatter uses spaces", "Write a short note in the pull request description",
"Where did I put my headphones", "The dashboard shows three agents waiting for approval",
"Could you call me back in an hour", "This function is way too long, split it up",
"Thanks, that fixed the problem", "Remind me to water the plants tomorrow",
"Hello everyone, welcome to the stand up", "Hi there, is this seat taken",
"Good morning, how did you sleep", "Hang on, someone is at the door",
"The doctor said to rest for a few days", "He played the cello in the school orchestra",
"Copper wire carries the current to the motor", "The concert was sold out in minutes",
"Let's conclude the review and merge it", "The condo downtown has a nice view",
"Configure the linter to ignore generated files", "Connect the monitor with the other cable",
"The construction noise starts at eight", "Consider caching the response for a minute",
"Can we discuss the contract terms on Thursday", "Say, could you pass the salt",
"Okay computer, play some music", "Hey, wait for me at the corner",
"Hey you, over here", "Hey, nice work on the demo",
"Hey, did the deploy finish", "Hey team, quick update on the release",
]
NEAR = ["The orchestra conductor raised her baton", "Copper is a good conductor of heat",
"Hey Connor, did you push the branch yesterday", "Let's conduct a quick review of the pull request",
"The train conductor checked our tickets", "Semiconductors are used in every laptop",
"The lightning rod is a conductor", "Hey, conduct yourself properly", "Who was the conductor of that concert",
"He conducted the experiment twice", "Conduct, conductor, conducting", "I'll ask the conductor when we arrive",
"Hey, can ducks swim in cold water", "Hey, construction starts tomorrow", "Hey, consider adding a test for that",
"Hey Doctor, the results are in", "Hey, come back here", "Hey Siri, set a timer for ten minutes"]

def corpus(name, sentences):
    segs, spans, at = [], [], 0
    for t in sentences:
        x = say(t, int(rng.integers(0, 900)), float(rng.uniform(0.85, 1.15))) * float(rng.uniform(0.5, 1.0))
        spans.append((at, at + len(x), t)); segs.append(x); at += len(x)
        gap = np.zeros(1600, np.float32); segs.append(gap); at += len(gap)
    write(f"{out}/{name}.wav", np.concatenate(segs), 1)
    with open(f"{out}/{name}.txt", "w") as f:
        f.write(f"# start end text (samples at 16 kHz in {name}.wav)\n")
        for s, e, t in spans:
            f.write(f"{s} {e} {t}\n")

# Everyday speech for the 30-minute clip; it never says the phrase.
corpus("speech_corpus", GENERAL)
# Near-homophones, measured and reported on their own.
corpus("near_misses", NEAR)
