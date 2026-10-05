# Test fixtures

The integration harness (`src-tauri/tests/pipeline.rs`) plays these through the real pipeline.
The audio is generated rather than committed:

```
uv run tests/fixtures/build_fixtures.py
```

This needs ffmpeg on `PATH` and writes `tests/fixtures/generated/` (git-ignored).

| File | Content |
| --- | --- |
| `meeting.wav` | Chapters 4 and 5 back to back with a 3 s pause, 16 kHz mono (about 3 min) |
| `meeting.ref.txt` | Reference transcript (sections 四 and 五) |
| `two_channel.wav` | Left = mic (自分), right = app (相手). While the right channel speaks (chapter 4), the left carries a faint, 40 ms delayed copy of it (speaker bleed, about −22 dB); then the left speaks (chapter 5) |
| `two_channel.others.ref.txt` / `two_channel.me.ref.txt` | Reference per channel |
| `english.wav` | One minute of English speech, 16 kHz mono, for the check that Whisper does not translate English while 日本語 is set (no reference: the test only asks for English text) |

## Sources

| What | Source | Licence |
| --- | --- | --- |
| Audio | LibriVox recording of 新美南吉『ごんぎつね』, archive.org item [`gongitsune_um_librivox`](https://archive.org/details/gongitsune_um_librivox), chapters 4 and 5 (`gongitsune_04_niimi_64kb.mp3`, `gongitsune_05_niimi_64kb.mp3`) | [CC0 1.0](http://creativecommons.org/publicdomain/zero/1.0/) |
| Reference text (`ref/gon4.txt`, `ref/gon5.txt`) | 青空文庫 図書カード No.628『ごん狐』 (新美南吉, died 1943) | Public domain |
| Benchmark clip (`src-tauri/resources/bench_ja.wav`) | Same recording, chapter 4, 4.0–14.0 s | CC0 1.0 |
| English audio | LibriVox recording of Jonathan Swift, "A Meditation Upon A Broomstick", archive.org item [`nonfiction008_librivox`](https://archive.org/details/nonfiction008_librivox) (`meditation_upon_a_broomstick_swift_tc_64kb.mp3`), first 60 s | [Public domain](http://creativecommons.org/licenses/publicdomain/) |

The script verifies each download against a pinned SHA-256 and cuts the LibriVox preamble and
outro (「リブリボックス…のために録音されました」, 「この録音はパブリックドメインです」),
which are not in the reference text. The spoken section numbers (四, 五) stay in both.

## Metric

Character error rate after NFKC with punctuation and whitespace removed. The pass criterion is
relative: streaming CER may be at most 3 points worse than transcribing the same file in one
pass with the same model, so reference quirks (for example the reader saying ごんぎつね where the
text has ごん狐) affect both sides equally.
