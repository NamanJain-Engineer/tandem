# Wake-phrase test clips (F-CN-07 AC1)

Synthetic speech from `scripts/gen-wake-fixtures.py`: Piper's LibriTTS-R
voice (904 speakers; the LibriTTS-R dataset is CC BY 4.0, openslr.org/141)
through sherpa-onnx TTS. They're test fixtures only and never ship.

- `hey_conductor_00..09.wav`: "Hey Conductor" by ten speakers held out of
  tuning, 16 kHz 16-bit.
- `speech_corpus.wav` + `.txt`: everyday sentences that never say the phrase
  (8-bit), stretched into the 30-minute benchmark clip by
  `wake::bench::speech_noise`.
- `near_misses.wav` + `.txt`: near-homophones ("hey Connor", "the conductor",
  "hey, conduct yourself"), measured and recorded but not part of the bar.

VITS synthesis is random, so running the script again gives different clips:
these files are the record the committed benchmark was measured on.
