# /// script
# requires-python = ">=3.11"
# ///
"""Builds the integration-test fixtures (section 18) from CC0 / public-domain sources.

Run from the repository root:  uv run tests/fixtures/build_fixtures.py
Needs ffmpeg on PATH. Output goes to tests/fixtures/generated/ (git-ignored).

Sources (see tests/fixtures/README.md):
  - Audio: LibriVox recording of 新美南吉『ごんぎつね』, CC0 1.0, archive.org item
    gongitsune_um_librivox (chapters 4 and 5).
  - Reference text: Aozora Bunko card 628 (public domain), sections 四 and 五, in ref/.
  - English speech: LibriVox recording of Jonathan Swift, "A Meditation Upon A Broomstick",
    public domain, archive.org item nonfiction008_librivox.
"""

import array
import hashlib
import pathlib
import subprocess
import sys
import urllib.request
import wave

HERE = pathlib.Path(__file__).resolve().parent
CACHE = HERE / "cache"
OUT = HERE / "generated"
RATE = 16_000

# (file, sha256, story start s, story end s): the LibriVox preamble and outro are cut off.
CHAPTERS = {
    4: ("gongitsune_04_niimi_64kb.mp3", "c0d26884e92ffcd58dd5f0c7b2b962b92b80236723eb0441300c87a585240847", 4.0, 110.5),
    5: ("gongitsune_05_niimi_64kb.mp3", "dd73097db911af6064e1ae4cb9f746809c457a0967843ced6a226b125cd95443", 3.9, 70.9),
}
BASE_URL = "https://archive.org/download/gongitsune_um_librivox/"
# English speech, for the check that Whisper does not translate it while 日本語 is set: the first
# minute (the LibriVox preamble, then the essay).
ENGLISH = ("meditation_upon_a_broomstick_swift_tc_64kb.mp3", "fc6a324bb4f9ed88eaf49872a614ed8ecd4ff963c37b679ce11fe70e89c0dc87", 0.0, 60.0)
ENGLISH_URL = "https://archive.org/download/nonfiction008_librivox/"
PAUSE_S = 3.0
ECHO_GAIN = 0.08  # about -22 dB of speaker bleed into the mic
ECHO_DELAY_S = 0.04


def fetch(name: str, sha256: str, base_url: str = BASE_URL) -> pathlib.Path:
    CACHE.mkdir(parents=True, exist_ok=True)
    path = CACHE / name
    if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest() != sha256:
        print(f"downloading {name}")
        data = urllib.request.urlopen(base_url + name, timeout=120).read()
        digest = hashlib.sha256(data).hexdigest()
        if digest != sha256:
            sys.exit(f"{name}: checksum mismatch ({digest})")
        path.write_bytes(data)
    return path


def decode(mp3: pathlib.Path, start: float, end: float) -> array.array:
    """16 kHz mono 16-bit samples of [start, end) seconds."""
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-ss", str(start), "-to", str(end), "-i", str(mp3), "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"],
        check=True,
        capture_output=True,
    ).stdout
    samples = array.array("h")
    samples.frombytes(raw)
    return samples


def silence(seconds: float) -> array.array:
    return array.array("h", bytes(int(seconds * RATE) * 2))


def write_wav(path: pathlib.Path, channels: list[array.array]) -> None:
    n = max(len(c) for c in channels)
    for c in channels:
        c.extend([0] * (n - len(c)))
    if len(channels) == 1:
        interleaved = channels[0]
    else:
        interleaved = array.array("h", [0]) * (n * len(channels))
        for ch, c in enumerate(channels):
            interleaved[ch :: len(channels)] = c
    with wave.open(str(path), "wb") as w:
        w.setnchannels(len(channels))
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(interleaved.tobytes())
    print(f"wrote {path.relative_to(HERE)} ({n / RATE:.1f} s, {len(channels)} ch)")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    story = {k: decode(fetch(f, sha), start, end) for k, (f, sha, start, end) in CHAPTERS.items()}
    ref = {k: (HERE / "ref" / f"gon{k}.txt").read_text(encoding="utf-8") for k in CHAPTERS}

    # A 3-minute "meeting": two speakers' turns with a pause between them.
    meeting = story[4] + silence(PAUSE_S) + story[5]
    write_wav(OUT / "meeting.wav", [meeting])
    (OUT / "meeting.ref.txt").write_text(ref[4] + ref[5], encoding="utf-8", newline="\n")

    # Two channels: left = mic (自分), right = app (相手). While 相手 talks, the mic picks up a
    # faint, slightly delayed copy of the speakers (echo); then 自分 talks.
    right = story[4] + silence(PAUSE_S + len(story[5]) / RATE)
    delay = int(ECHO_DELAY_S * RATE)
    echo = silence(ECHO_DELAY_S) + array.array("h", (int(s * ECHO_GAIN) for s in story[4]))
    left = echo[: len(story[4]) + delay] + silence(PAUSE_S) + story[5]
    write_wav(OUT / "two_channel.wav", [left, right])
    (OUT / "two_channel.others.ref.txt").write_text(ref[4], encoding="utf-8", newline="\n")
    (OUT / "two_channel.me.ref.txt").write_text(ref[5], encoding="utf-8", newline="\n")

    name, sha, start, end = ENGLISH
    write_wav(OUT / "english.wav", [decode(fetch(name, sha, ENGLISH_URL), start, end)])


if __name__ == "__main__":
    main()
