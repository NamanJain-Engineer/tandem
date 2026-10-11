# Wake-phrase model (F-CN-07)

The "Hey Conductor" keyword spotter, bundled with the app and run by the
`tandem-wake` sidecar.

- **Model:** `sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`, a
  3.3M-parameter streaming zipformer transducer trained with icefall on
  GigaSpeech (English, BPE units). The int8 encoder, decoder and joiner and
  `tokens.txt`, renamed; nothing else changed.
- **Source:** https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2
  (SHA-256 `f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a`).
- **Licence:** Apache-2.0 (the model card). The licence review is in
  `HUMAN_TODO.md`.

| File | SHA-256 |
|---|---|
| `encoder.int8.onnx` | `1e721676515bcd42a186979733981213c66c80db680e1cc582dfedf3be76e678` |
| `decoder.int8.onnx` | `e40ff43297abe815e8898494c17e71bba2152d9d40fa3eb803f75d0f7533329a` |
| `joiner.int8.onnx` | `eae9da0c7e1e6c6a3f4cc42d167899c388f6c6701b94cb96320e4f55df79624c` |
| `tokens.txt` | `fd2ded4050a55d2b1578870ba8697d02371980217806b7558bd0a5cc60f3ba53` |

`benchmark.json` records the false-accept benchmark
(`cargo test -p tandem-voice --features kws -- --include-ignored f_cn_07_1`).
The app offers the wake phrase only while it says `passed`. The keywords and
thresholds are in `src/wake/sherpa.rs` and `Sensitivity::tuning`.
