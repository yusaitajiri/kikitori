# Decisions

Choices made where the spec (`AppSpec.md`) is silent, and the few places where implementation
found a reason to deviate.

## Build and toolchain

- **Tauri 2.12.1**, the latest 2.x release. Tauri 3 is still alpha.
- **Exact pins.** Drift-prone crates (`tauri*`, `whisper-rs`, `wasapi`, `xcap`, `earshot`,
  `rubato`, `ulid`, `typst*`, `zip`) are pinned with `=`; everything else is pinned by
  `Cargo.lock` and `pnpm-lock.yaml`, which are committed.
- **Hand-written IPC types** (`src/ipc/*.ts`) instead of `tauri-specta`, which is still a
  release candidate (2.0.0-rc.25). The Rust side is the source of truth; field names are camelCase.
- **whisper.cpp optimization on MSVC.** The `cmake` crate replaces CMake's per-config flags with
  `cc`'s, which carry no optimization level on MSVC, so whisper.cpp built unoptimized: model load
  went from 9 s to 84 s and CPU RTF from 0.39 to 3.5. `src-tauri/.cargo/config.toml` sets
  `CMAKE_{C,CXX}_FLAGS_RELEASE` back to `-O2 -Ob2 -DNDEBUG`, and the same for
  `RELWITHDEBINFO`, the config dev builds use (they were unoptimized too).
- **Static C runtime.** `.cargo/config.toml` adds `-C target-feature=+crt-static` for the MSVC
  target and passes `-MT` to whisper.cpp to match (the flags above replace the `cmake` crate's,
  so it can't). The release exe imports no `vcruntime140.dll` or `msvcp140.dll`; it imports
  only the Universal CRT (`api-ms-win-crt-*`), which is part of Windows 10 and 11.
- **Vulkan loader bundled.** whisper.cpp's Vulkan backend links `vulkan-1.dll`, which only GPU
  drivers install, so the exe would not start on a PC without one. `npm run build:gpu` downloads
  LunarG's `VulkanRT-X64-1.4.363.0-Components.zip` (pinned SHA-256 for the zip and the DLL) into
  the git-ignored `src-tauri/resources/vulkan/`, and `tauri.gpu.conf.json` installs
  `vulkan-1.dll` next to the exe with its licence in `licenses\`. With no driver the loader
  reports no devices and the app runs on CPU.
- **Installer size: about 22 MB**, over the spec's 15 MB target (section 16). Most of the
  difference is the Typst compiler linked in for PDF export: before it, the installer was
  12.6 MB.
- **CI.** Actions are pinned to exact tags and Rust to 1.98.1 (the version the code was built
  with). The short target directory is computed at run time (`$GITHUB_WORKSPACE\..\t`) instead
  of assuming a drive letter. `ci.yml` only checks, and skips pushes that change nothing but
  Markdown or `docs/`; installers come from `release.yml`, which builds one per `v*` tag.
- **rustfmt** uses `max_width = 120` with `use_small_heuristics = "Max"`, the style the code was
  written in; `cargo fmt --check` runs in CI.
- **Licence: MIT**, the spec's default for a public repository.
- **Third-party notices are generated at build time.** MIT, BSD and Apache-2.0 ask for their
  notices to travel with the binary, so `build:gpu` runs `scripts/licenses.mjs`, which writes
  `licenses\THIRD-PARTY-NOTICES.txt` (about 1 MB): each distinct licence text once, with the
  packages that ship it. It covers the crates linked into the exe (from `cargo metadata`; build
  scripts and proc macros only run while compiling), whisper.cpp (MIT, inside whisper-rs-sys),
  and the bundled frontend packages plus Tailwind's base styles. Packages that ship no licence
  file are listed with their declared licence and source. Generating it rather than committing
  it means it can't go stale; a CPU `tauri build` leaves it out, like the Vulkan loader. Typst's
  bundled fonts are not compiled in (`typst-assets` without `fonts`); hayagriva's CSL styles
  (CC BY-SA 3.0) are, and its notice in the file covers them.
- **CPU baseline: AVX2.** Local builds use `GGML_NATIVE` (AVX-512 on a Zen 5 CPU). Release
  builds set `GGML_NATIVE=OFF`, which selects `/arch:AVX2`, because a binary built with
  `/arch:AVX512` crashes on most consumer Intel CPUs. At startup the app checks for AVX2 and FMA
  and shows 「このPCのCPUは文字起こしに対応していません」 instead of crashing.
- **Vulkan builds need a short target directory.** ggml-vulkan builds `vulkan-shaders-gen` as a
  nested CMake project; under a normal checkout its MSBuild FileTracker paths pass 260 characters
  (error FTK1011). `npm run dev:gpu` / `npm run build:gpu` default `CARGO_TARGET_DIR` to
  `%USERPROFILE%\.kt`; CI uses a short path too. The Vulkan SDK is needed only to build.
- **`beforeDevCommand` / `beforeBuildCommand` use `npm run`**, which works wherever Node is
  installed; pnpm is reached through corepack and may not be on `PATH`.
- **Linting** uses oxlint (the Vite template's default) rather than ESLint.

## Transcription

- **Whisper states are freed between recordings.** Each source's `WhisperState` holds about
  280 MB of VRAM (turbo-q5) and 700 MB of commit. The worker drops them once its queue is empty
  after a session is saved, a mic test or a benchmark, and recreates them on the next job.
- **GPU device name** comes from `whisper_rs::vulkan::list_devices()` (device 0, the one
  whisper.cpp uses) instead of parsing whisper.cpp's log output.
- **Flash attention** is on for GPU contexts (whisper.cpp's current default); a failure falls
  back to CPU like any other GPU init failure.
- **Remembered GPU failure** is keyed by model ID and GPU name; whisper.cpp does not expose the
  driver version. Settings → モデル has 「GPUを再試行」 to clear it.
- **GPU crash detection.** Before a GPU init the app writes `gpuInitInProgress` to `state.json`
  and clears it afterwards. If it is still set at launch, the init crashed the process, so the
  app records a GPU failure and uses the CPU.
- **Backlog merging.** Inside the worker, "lag ≥ 5 s" is measured as the span between the oldest
  and newest waiting utterance of a session, which needs no clock and only matters when there is
  more than one job to merge.
- **Language 自動.** Utterances are decoded with language `auto` until one of 3 s or longer has
  been decoded; its detected language is then locked. The session keeps `language: "auto"`; the
  detected language is logged as a `language_detected` event.
- **Whisper never translates** (`asr/language.rs`). Told a language the speech is not in,
  Whisper translates into it or loops: with 日本語 set, English speech came out in the grey
  partials as 「このグリースを選択して、押し込みを…」, and a final without a prompt as
  「私は、私は、…」 (finals mostly stayed English only because of their prompt). After each decode
  the worker runs one decoder step on the start-of-transcript token over the encoder output the
  decode left in the state, which is how whisper.cpp detects a language, without a second encoder
  pass (5 ms on CPU; the probabilities match whisper.cpp's own detection). When Whisper is at
  least 0.9 sure a clip is in the other of Japanese and English, the clip is decoded again in that
  one, and the source's next clips start in it while Whisper stays at least 0.5 sure; then they go
  back to the configured language. On real speech, Japanese scored at most 0.74 for English (0.6 s
  fragments and silence, turbo-q5 and base-q5) and English at least 0.9 (turbo-q5: 0.999). Clips
  Whisper is unsure of keep the fixed language, as section 8 asks; 自動 works the same way once it
  has locked.
- **Primer** 「えー、それでは始めます。…」 is used only when decoding Japanese.
- **Partials** run only on GPU, when no final job is queued or running, and when the model's
  benchmark tier is 快適 (or no benchmark has run).
- **Segmenter details.** The utterance keeps up to 320 ms of trailing silence (capped by the
  hangover). Pre-roll never reaches back into the previous utterance, so no audio is transcribed
  twice. The soft maximum cuts at the middle of the quietest 6-frame (96 ms) window in the last
  3 s; the hard maximum is checked before a frame is appended so no utterance exceeds 25 s.
- **Echo guard.** A 自分 line that matches an earlier 相手 line is never written; one already
  written is removed with `segment_removed` when its 相手 match arrives. Besides pairwise Dice
  similarity, the 自分 line is compared with all overlapping 相手 lines joined (the share of its
  bigrams found there), because the mic's VAD often cuts one echo line across several 相手
  lines. With turbo-q5 this took the echo left in the two-channel fixture from 82 characters to 0.
- **Short utterances get no context prompt.** With under 1 s of speech the prompt is the word
  list only: given the previous line, Whisper tended to repeat it (a lone 「四」 came out as
  「よろしくお願いします。」).
- **Mic test** runs the VAD and segmenter over its 10 s recording and transcribes only the
  speech, so a silent test shows 「言葉は聞き取れませんでしたが、続けて大丈夫です」 instead of a
  hallucinated 「ありがとうございました。」.
- **Hallucination list** is matched after NFKC; half-width katakana normalizes to katakana, so
  hiragana phrases do not match their katakana forms.

## Audio capture

- **App picker** lists sessions on every active render device, not only the default one, so an
  app playing to a headset that is not the default still shows up.
- **App capture can start before the app runs.** The capture thread waits for a process with the
  same executable (the reattach loop of FR-14), so the Start hotkey works with a last-used app
  that is closed. The status row says 「Zoom の音声を待っています」.
- **Default-device changes (FR-15)** are debounced for 1.5 s and reopen the stream only if the
  default actually changed. Opening a Bluetooth hands-free mic makes Windows flap the default
  while the headset switches profiles; reopening on every notification thrashed.
- **Device switches** show a notice but no timeline marker; the 「音声ソース再接続」 marker is
  for an app that exited and came back.
- **`read_from_device`** with an exactly sized buffer is used instead of
  `read_from_device_to_deque`, which unwraps the `ReleaseBuffer` result inside `wasapi`.
- **Missing QPC timestamps.** A packet with timestamp 0 or the timestamp-error flag is placed at
  "now minus its duration".
- **Gaps are never materialized.** The clock reports gaps as a frame count and the DSP thread
  feeds silence in half-second pieces, so an app that was closed for minutes costs no memory.
- **Quiet streams.** System loopback delivers no packets in silence; after 500 ms without packets
  the DSP thread pads silence up to 300 ms before now so an open utterance can close.
- **Health checks.** "No packets for 5 s" restarts mic and app streams once; only the mic then
  raises a banner, because process loopback can legitimately go quiet. System loopback is exempt.

## Sessions and exports

- **`session.json`** also stores the SHA-256 of files Kikitori wrote (`written`), which is how
  an edited `transcript.md` is detected and never overwritten.
- **Rename** changes the title only; the folder keeps its original name.
- **Projects live beside the sessions** (FR-64): `projects.json` in the output root holds each
  project's ID, name and colour, and a session logs only the ID (`project_set`), so the list
  moves with the folders and a rename shows everywhere at once. Deleting a project leaves its
  sessions' IDs in place, naming nothing, so they read as in no project and no session folder is
  rewritten. The Markdown front matter looks the name up beside the folder.
- **A cut is a marker** (`cut`, FR-06), not a new session: the part shares the session's
  sound, line and folder, and a cut needs no new event in the log. It works while paused, so
  a break can end a part. Markdown heads the part with `## HH:MM:SS`, the PDF with the time
  over a hairline as a heading (so the PDF outline lists the parts), and plain text with
  「— HH:MM:SS 区切り —」. Parts have no names yet; the marker's `detail` would hold one.
- **"The line being said" is an utterance, not a time** (FR-07). A line's text arrives seconds
  after it is spoken, so the star cannot just mark the newest line: while someone speaks that
  is the line before. Each DSP thread notes the utterance it has open and the last one it
  closed (`SessionCtx::speaking`); the star takes every utterance open at that moment, else the
  one that ended last, and the store keeps the mark until that utterance's text arrives. Whisper
  may split one utterance into several lines; the mark goes to the one spoken at the moment of
  the press. An utterance that gives no line (too short, a filtered hallucination) drops its
  mark. Marking from the transcript works on any line, live or after, through `mark_segment`.
- **Important lines in exports** start with `★ `, a character an agent reads as plainly as a
  person, rather than bold or highlight markup, which differs between Markdown flavours and
  would be lost in plain text. An important line is its own paragraph, so the star never
  claims a neighbour's words.
- **Switching the source is a new capture plan, not a new session** (FR-17). Capture stops,
  the DSP threads flush their last utterances, and the new plan starts at the same session time,
  as a resume does; the session adds the new source to its list (so two apps may both be 相手)
  and logs a `source_changed` marker naming it in Japanese, like every export string. 相手 and
  自分 labels and the echo guard now ask whether both sides were ever recorded
  (`Session::two_sides`), not how many sources there are, so Zoom → Teams alone stays unlabelled.
  If the new source fails, the old plan restarts and the picker goes back to it. While paused
  the new plan waits for resume. 「システム全体に切り替える」 after a silent app is the same switch.
- **Continuing a session appends, it does not merge** (FR-08). The new recording starts at the
  next 100 ms step after the session's end, so the transcript, the screenshots and `levels.bin`
  carry on in one session time, and each continuation records its wall-clock start
  (`Session::continued`) so the clock times after it stay true even days later; the timeline
  marks the seam with a `continued` marker (with the date when it differs). Session zero then
  lies the session's length before now, which may be before the PC started, so the recorder's
  and the stream clock's `t0` are signed. IDs carry on after the highest each kind used in the
  log, removed lines included. `session_continued` is logged only once capture runs, so a
  source that fails leaves the session as it was; a session that ended in a crash is recovered
  first. The title, project and model of record stay the session's; the source is whatever the
  picker holds now, and new sources join its list.
- **Unprocessed audio** after 処理を中止 is recorded as an `unprocessed` marker and
  `unprocessedMs`, rendered as 「（以降、未処理の音声 N 秒）」.
- **Exports are Japanese** in any UI locale: the session stores 自分 / 相手 and the spec defines
  the export strings in Japanese.
- **Joining text.** Besides the spec's Latin-letter/digit rule, a space is inserted after Latin
  sentence punctuation (`. , ! ? ; :`) before a Latin letter, so English sessions read naturally.
  Japanese joins are unaffected.
- **Markdown escaping.** `\ * _ ` [ ] < >` are escaped in transcript text, and a paragraph that
  would start a heading or list is escaped. Front-matter values are YAML-quoted when needed.
- **PDF through Typst** (section 11). The `typst` crate compiles the PDF in process from
  `export/transcript.typ`, so no hidden print window is needed. The session becomes a Typst
  document: the preamble, then one call per block (`#entry`, `#shot`, `#note`) with every piece of
  session text in a string literal, so none of it can be read as Typst syntax. Calls rather than
  a JSON data file keep the exported `.typ` readable. The preamble alone decides the look: A4,
  18 mm margins, Yu Gothic (Meiryo and MS Gothic if missing), 相手 in red, and a footer with the
  app's name, small, beside the red bird. Fonts are read from `%WINDIR%\Fonts` for each export
  rather than kept in memory (about 15 ms), and Typst's memo cache is cleared after each one. A
  one-minute session with a screenshot renders in about 0.2 s (debug build) to 330 KB. Typst is
  Apache-2.0: its LICENSE and NOTICE are installed in `licenses/`. The print HTML stays only as
  the fallback, opened in the browser when Typst fails.
  `cargo run --example export_pdf -- <folder> <out.pdf>` renders a session without the app.
- **Typst file.** 書き出し → Typst saves that same document, so `typst compile` gives the app's
  PDF (checked with the standalone Typst 0.15 compiler on a real session saved under a name with
  a space).
- **Markdown is one option with a ZIP switch.** Off, the user picks a directory and gets a new
  folder named after the session (`-2` if taken) holding `transcript.md` and `images/`, the
  session folder's own layout, so links stay `images/…`. On, the same two go into one ZIP (no
  PDF), written through a `.zip.part` file so a failure leaves nothing half-written. The switch
  is remembered in the WebView's local storage; the compact window's menu has both as separate
  entries.
- **Every export asks where to save it** and none is written into the session folder: a save
  dialog over the window, starting in Downloads, suggests the session folder's name. Typst
  copies its screenshots into `<name>_images/` beside the file (named after it, so an `images/`
  already there is untouched) and links them there. The PDF opens when saved; the others only
  say where they went. If Typst fails, the print HTML goes where the PDF was to go, its
  screenshots as `file:///` links into the session folder. `transcript.md` is still written into
  the session folder on Stop: it is the recording's own copy, not an export.

## UI

- **Look: one dot, one line.** The app's one original idea is the dot that becomes a line across
  the window, so nothing else competes with it: no stock parts (chips, stat tiles, icon tiles,
  pill tabs, a phone-style tab bar), no pink tints doing the identity work, no decorative waves
  that would make the live line read as decoration, and a finished transcript gets most of the
  window. Three colours: white and neutral greys, near-black ink (also the everyday accent) and
  true red `#D8232A` (dark mode `#FF3B3F`, which keeps the hue on near-black and gives 相手's name
  5:1; `#FF5B5B` read salmon). **Red is the voice you hear**: 相手's line and name, the start dot
  and 停止. 自分 is ink. Alerts (banners, 要復元) are inverted, paper on ink, so red never means an
  error. No glows, gradients, blur or pink layers; the only tint lies under the line while
  recording, and a hint of it after.
- **The line is real sound** (`components/Deck.tsx`, `lib/line.ts`, `store/line.ts`). While
  recording, the line across the window is drawn from the levels the backend sends ten times a
  second: 相手 rises above it in red and 自分 dips below it in ink (a single source rises above,
  whichever it is), lightly blurred so a lone step is not a spike. A pen tip moves right from
  where the start dot was and, near the end, the line scrolls under it. Screenshots are dots on
  the baseline and a pause leaves a dotted gap. While finishing, the steps not yet written are
  grey and fill in as the queue empties, with the pen waiting where the written part ends. The
  steps live outside React (`lineHistory`, fed in `App.tsx`), so the line survives a visit to
  History or Settings. The line is the edge of the tint, so it runs from edge to edge: the pen
  starts in line with the buttons under it (14 px, compact 12), the tint rises to the top of the
  line, and where nothing is written, before the pen started and ahead of it, the line is grey
  and as heavy as the written one, coming back down to the baseline just after the pen. Each
  stretch rises from the baseline where it starts and drops back to it before a pause, so the
  line never ends in mid-air. These are FR-03's per-source meters: a decorative wave would look
  the same in every meeting and could not show a silent mic.
- **A session's picture is its sound.** A shape built from when each side spoke is flat in
  continuous speech, where every slice has speech, so the recorder keeps the 10 Hz levels in
  `levels.bin` (2 bytes per 100 ms, paused time silent), and History and the finished session
  draw 128 slices of mean loudness, raised together so the loudest stands full height (at most
  2×). A session without `levels.bin` falls back to when each side spoke (`shapeOf`; Rust
  computes the same as `SessionSummary.activity` for History).
- **Every session keeps its shape.** When a session is saved the pen lifts and the line zooms out
  to the whole session, with dots for screenshots. Pointing at it shows the clock time; clicking
  scrolls the transcript there and marks the row (`seek` in the transcript store). The same shape
  is the session's picture in 履歴.
- **One dot.** Before a recording the expanded window has no transcript to show, so the deck
  takes the free space under the source picker, and the red dot sits in its middle with 開始
  written on it: centred in the space above the model's line at the bottom and lifted a little
  to the optical centre (45% of that height), because at the exact middle it looks low. It is
  three layers: the dot on two rings with wavy rims, each a little bigger and paler than the one
  inside it, which stir and swell under the pointer. There is no line yet; a hotkey, when one is
  set, sits under the rings, and the model at the bottom edge, so nothing pulls the dot off
  centre. Starting (button or hotkey) sends the dot down to the bottom's left end, where it
  shrinks into the pen tip while its rings fold into it and the line appears; the deck eases from
  its full height to its strip as the transcript opens above it. The deck stays mounted from the
  start screen to the finished session, so no cross-screen morph is needed. 「新しい録音」
  flattens the session's line and the dot rises back to the middle on its rings. The compact
  window has no free space: its dot waits at the left end of an empty line, 開始 beside it. The
  dot is the app's only circle (its rings are wavy); switch knobs are rounded squares.
- **The mark is a bird.** Kikitori reads as 聞き + とり, and とり is a bird, so the red dot became a
  red bird: a round head with a pointed tail, an ink beak and an eye in the background colour.
  The app icon (`src-tauri/icons/icon.svg`, the source of every size `tauri icon` writes) sets it
  on the white tile with its outline rippling out in three fading wavy layers, the start dot's
  rings carried over. The logo (`docs/images/kikitori-logo.svg` and its dark twin) is `kikitori`
  in round strokes with the bird as its o. The title bar shows the logo (`components/Logo.tsx`,
  the same drawing in the theme's ink and red, so it follows light and dark), and the PDF footer
  draws the bird. The tray shows the bird alone when idle, with its red ripples while recording
  and grey ones while paused or finishing (`icons/tray/`). Centred red-dot icons were considered
  and dropped as too common.
- **What to listen to is a source and a conversation switch.** The source opens one list:
  システム全体, マイクだけ, then the apps (those playing first, その他 after, with a refresh
  button, FR-11). Under it, 「会話として録音」 is FR-12's mic switch, worded as what it is for,
  and its own list picks the mic (the same setting as 設定 › 音声). The names 相手 and 自分 appear
  only in the transcript: on the switch they would mislead for recordings that are not a
  conversation (a lecture, a video). With マイクだけ the second row only picks the mic. The
  compact window shows the choice as one line (`Zoom · 会話`) that opens the picker over the
  whole window, and loads the app list when it appears, so the line carries the app's icon.
- **Apps show their sound.** In the source picker each app with an audio session shows its sound
  beside its name while it plays (`AppWave`): the last two seconds as a small red line on a grey
  baseline, in the field and in the list. It is real sound, not a decorative wave: the peak
  meters Windows keeps for every audio session (the volume mixer's bars), read by
  `audio::win::meters` only for the apps on screen, twenty times a second, with nothing captured
  and nothing sent in silence. A read takes about 30 µs and listing the sessions about 2 ms, so
  one app costs ~0.15% of a core. The wave steps ten times a second instead of gliding every frame
  like the recording's line: redrawn at the screen's rate it cost about 40% of a core on a 165 Hz
  screen (Chromium, measured), stepping about 4%, and nothing once it is quiet.
- **Navigation is words in the title bar**: 録音 · 履歴 · 設定 in the expanded window, the current
  one underlined, 録音 with a red dot while recording; the compact window keeps its icons. While
  recording the title bar shows the state and the source in place of the name, and away from the
  main view that leads back to the recording. No bottom tabs: a phone pattern that would add a
  second way to navigate, the largest coloured area in the app, and a second layer of controls
  under the deck while recording.
- **One title bar.** It is one element over every view (`App.tsx` renders it outside the view it
  keys by place), so switching 録音 · 履歴 · 設定 never rebuilds it; the back arrow follows the view
  (`lib/nav.ts` `backOf`). The compact toggle is on every page but the wizard: only 録音 has a
  compact form, so shrinking from 履歴 or 設定 goes to 録音.
- **White headers.** History, Settings, the model list, the wizard and a finished session open
  with a big title on white above a hairline. A finished session's header scrolls with its
  transcript: the title (click to rename), when it started, its length, lines and screenshots in
  big light numerals, and 詳細, closed at first, for the model and the folder it was saved to. A
  finished transcript gets 476 of 640 px. The header doesn't say where the session was saved
  (「保存しました · Kikitori › …」 read like a garbled 「Kikitori › … に保存しました」); saving
  shows a toast instead.
- **Settings is a list** of its sections, each showing what it is set to now; a section opens with
  a back arrow. A pill tab per section would overflow the 420 px window with no sign that the
  tabs scroll.
- **Projects are coloured squares** (FR-64), the one place colours beyond white, ink and red
  appear, so they stay small: a 10 px rounded square before a title, never a fill, a chip or a
  tinted row. Squares, because the dot is the app's only circle. Seven muted colours, light and
  dark shades, none of them red (red is the voice you hear); a new project gets one no other
  project uses. A session joins a project from the swatch beside the recording's title, the line
  under a finished session's title, History's row menu (a submenu) or 選択 (a button beside
  削除), each a native menu ending in 「新しいプロジェクト…」. History filters by project with a
  menu button beside the search, shown only once a project exists; 設定 › プロジェクト lists
  them to rename, recolour and delete. A new recording starts in no project: one tap on the
  swatch puts it in one, and a default that carried over would file recordings by surprise.
- **History is grouped by day** (今日 and 昨日 with the date, then dates) and searchable by title
  and opening words (`SessionSummary.preview`), each row with its title, time, line, opening
  words and length. A recording shorter than 10 s or with no transcript (most likely a test or
  started by accident) is greyed out and has no line; it still opens, and its menu still deletes
  it. 選択 picks several recordings to move to the Recycle Bin at once (`delete_sessions`), with
  quick picks for all and for the greyed short and empty ones.
- **続きを録音 is in the ⋯ menus** (a finished session's and History's rows), not a button of its
  own: the finished session's row already holds four buttons in the compact window, and
  continuing is rarer than starting anew. It records with the source the picker holds and opens
  the recording view; while something records, it is greyed out.
- **A finished session's buttons**: コピー, 書き出し, which opens a dialog to pick Markdown, PDF
  or Typst (the compact window, too small for it, shows a menu), the ⋯ menu (「Agent用にコピー」,
  「Markdownでコピー」, フォルダを開く), and 「新しい録音」.
- **Less text.** Choices have no descriptions under them (the source field is one line,
  「会話として録音」 shows only the mic's name, the export dialog lists formats only, settings
  have no help lines, the wizard's paragraphs are one short line or none), and labels are short
  (「ソース」). Pages and wizard steps change without a slide.
- **Always on top is off by default**, toggled by a pin in the title bar on every page.
- **Screenshots take the recorded app by default**: its window while recording an app, the
  cursor's screen while recording system audio or the mic alone (`appWindow`).
- **The camera has a menu** (a chevron joined to it, FR-34), a native popup like ⋯ so it fits
  the compact window. The three targets are the setting itself, so a choice there is
  remembered, as in 設定 › スクショ; a window from its list (up to 20, front first, Kikitori's own
  left out, from the same `xcap` listing) holds for the rest of the recording only, since a
  window seldom outlives one. A picked window that has closed or been minimized falls back to the
  cursor's screen, with the usual 「（カーソルの画面）」 toast. The picked window travels in
  `recording://state` (`shotWindow`), so the menu's check marks follow the backend.
- **Speaker names** are coloured text: 相手 red, 自分 ink. A darker red for 自分 would be hard to
  tell from 相手 and nearly identical with red-green colour blindness. Provisional text is plain
  pencil grey.
- **Hairlines part a screen.** The only waves are the sound and the start dot's rings. Rounded
  rectangles stay for cards, fields, menus and secondary buttons; the one filled button of a
  place (the ink primary, the red 停止) is a pill; dialogs are centred sheets with full-width
  pills. Transcript markers (pause, reconnect) keep a straight line.
- **Japanese line breaks** use `word-break: auto-phrase` (WebView2, with `lang="ja"`), so words
  such as ヘッドホン and ください no longer split across lines.
- **One animation loop** (`lib/frameLoop.ts`) runs only while something moves: the live line
  redraws every frame only while recording, so an idle window stays under the 1% CPU target.
  With Windows' animation effects off, the dot's changes jump to their end and the line steps
  ten times a second instead of gliding. It draws at most about 60 frames a second: on a 165 Hz
  screen the recording line was drawn 168 times a second and the page took about half a core;
  capped, it draws on every third refresh (55 a second) and takes about a quarter (Chromium,
  measured). The compact window, on screen through whole meetings, draws its line 30 times a
  second (an animation's `fps`): 12% of a core instead of 20%. What is left is the browser
  painting and compositing the redrawn line; building it in JavaScript is about 1% of a core.
  Minimized or in the tray nothing is drawn at all.
- **A minimized or hidden window tells WebView2 it is hidden** (`window::sync_webview_visibility`,
  `SetIsVisible(false)`). WebView2 otherwise keeps the page `visible` and the live line drawing
  (about 1.6% CPU, measured). Hidden, animation frames stop and timers slow down, but backend
  events still arrive, so the transcript keeps up. Tauri's `backgroundThrottling` option only
  applies to WebKit.
- **The transcript runs on under the deck to its line.** The line lies 30 px into the deck; if
  the transcript stopped at the deck's top, text would vanish at an invisible edge above the
  wave. Once there is a transcript the expanded deck reaches up under it by that much
  (`kk-deck-under`, a negative margin) and is see-through above the line: the text goes right up
  to the wave and only the tint under the line covers it. The deck lets the pointer through to
  the text, except on its buttons and, after a recording, a band around the line where pointing
  shows the time. Scrolled to the end, the last line still rests clear of the deck.
- **Transcript.** Lines are grouped into turns: with both 相手 and 自分 recorded the speaker,
  then the time, head each turn on its own line, and the text runs the full width (a new turn
  starts at a speaker change, after a picture or marker, and every minute). There is no time
  column: at 52 px it would push the text to x = 80 and wrap lines after about 23 characters.
  With one source there are no speaker names.
- **Screenshot placement is strictly chronological** (section 9). Placing a screenshot after
  every sentence containing its time put it after sentences that started later: Whisper often
  cuts one utterance into back-to-back segments, and a segment starting exactly at the
  containing one's end counted as inside.
- **最新へ jumps from far away.** More than two screens above the newest line it scrolls there
  at once instead of gliding: in a long session left in the background the glide took seconds,
  and rows measured on the way kept moving the end.
- **The recording keeps a header**: the title (click to rename, as in a finished session) and
  the source as one line (`Zoom · 会話`), above the transcript and outside its scroll, so both stay
  in view in a long session. It is small (one line, 15 px title) because the transcript is what
  the window is for. The source opens the same picker as before a recording, over the window
  under the title bar; while recording each change switches at once (one tap, no 適用 button),
  and a toast confirms it. The compact window has no room for a header, so its title bar's
  録音中 tag opens the picker; away from the main view the tag still leads back to the recording.
- **The star** sits between the camera and the cut while recording, and on every line's hover
  actions (live too, unlike editing). An important line has an ink stroke in the left margin
  and semibold text; red stays the voice you hear. The compact window's two lines show a star.
- **More hotkeys.** Important mark and cut have hotkeys too (off until set), since a meeting
  app is usually in front. Pressed from another app, they confirm with a toast; all four share
  one registration loop, and one set to the same keys as another is reported, not registered.
- **The cut button** (scissors) sits after the camera while recording. A cut shows in the
  transcript as a marker with a darker rule and more space above it, and a toast confirms it,
  since the compact window's two lines leave markers out.
- **The timer leaves out paused time** (`StatePayload.elapsedMs`); session times still count
  it, so the clock times in the transcript stay true.
- **Animations** are CSS plus the deck's own frames; what is above the line fades between the
  start screen and a session. Everything honours `prefers-reduced-motion`.
- **The session viewer has its own transcript store** (`createTranscriptStore`), so a
  recording that runs while a past session is open keeps receiving its lines.
- **The … menu is a native popup menu**, because the compact window (380×170) is too small to
  hold a dropdown and a webview cannot draw outside its window.
- **Popup menu items are created one by one** (`src/lib/popupMenu.ts`). Tauri 2.12 drops the
  Rust side of items written inline in `Menu.new({ items })` once the menu is built, and with it
  their click handlers, so the menu opened but no item did anything. Items made with
  `MenuItem.new()` stay in the resource table; they are closed when the next menu opens.
- **Compact source picker** opens as an overlay over the whole window under the title bar, in a
  portal so the deck's layers cannot cover it.
- **Closing the window** hides it to the tray only while recording or finishing; when idle it
  quits (FR-93 only covers the recording case).
- **Window position** is remembered with `tauri-plugin-window-state`; the size follows the
  layout. Settings, History and the wizard use the expanded size temporarily.
- **Hotkeys are off until set.** Both default to empty: obvious choices such as `Ctrl+Alt+R` and
  `Ctrl+Alt+S` are often held by another app already, and a warning at every launch would be
  worse than no shortcut. 設定 › ショートカット sets each one or turns it off again; an empty one
  is never registered. A shortcut that fails to register is reported only there (FR-92).
- **Shutter sound** (設定 › スクショ, off by default) is two short bursts of decaying noise that
  `screenshot::shutter_wav` builds at runtime, so the installer ships no audio file. It is played
  from Rust with `PlaySoundW` once the capture succeeds, so it also sounds while the window is in
  the tray.
- **Hotkey recorder** requires Ctrl or Alt (function keys may stand alone) so a shortcut can never
  swallow normal typing.
- **i18n placeholders** use single braces (`{n}`) to match the spec's string table.
- **Notices** carry an i18n key plus `params` and a `toast` flag so Rust never formats UI text.

## Languages

- **The first launch follows the Windows display language** (`GetUserDefaultUILanguage`):
  Japanese on a Japanese Windows, English on any other, so the setup wizard is already in the
  user's language. Once saved, the choice in 設定 › 一般 wins.
- **The UI names the sources.** The state the backend sends says which sources are recorded and
  the app's name (`StatePayload.sources`), and the title bar words them in the UI language
  (`Zoom + マイク`, `Zoom + Mic`); hovering shows all of it when the bar is too narrow. 「マイク」
  is the short form, so the English word is "Mic".
- **A new session's default title** names the source in the UI's words at the start: the app's
  name, システム全体 / All system audio, or マイク / Mic. The title is the user's to rename;
  exports stay Japanese (see Sessions and exports).

## Updates

- **Off until keys exist.** The updater plugin requires `plugins.updater.pubkey`, and the keys
  are the maintainer's secret, so the plugin is registered only when that config is present.
  `scripts/enable-updater.mjs <github-owner>` writes the public key, the GitHub Releases endpoint,
  `installMode: "passive"` and `createUpdaterArtifacts` after `tauri signer generate`.
- **Launch check** runs 10 s after launch (the window must be listening for the banner) and at
  most once a day; the time is recorded when the check runs, whatever its result. A found update
  is a banner 「新しいバージョン x.y.z があります」 with 「更新する」.
- **Install** is refused while recording or finishing, and the recorder lock is held from the
  last check until the installer starts, so a recording can't begin in between. The passive NSIS
  installer closes the app and starts it again (`/R`); the window position is saved first.
- `requireSignedVersion` is not enabled yet: it rejects signatures that don't carry the version,
  and it is unverified whether Tauri CLI 2.12.1 writes it.

## Security

- **Capabilities.** `build.rs` registers the app's commands with `AppManifest`, so each needs an
  explicit `allow-*` permission; `capabilities/default.json` grants the main window those plus
  `core:default` and window dragging. No plugin command is reachable from the webview: the
  clipboard, dialogs, opener, notifications and store are all used from Rust.
- **CSP** follows the spec and adds the Windows forms Tauri uses: `ipc:`/`http://ipc.localhost`
  for IPC and `http://asset.localhost` for screenshot thumbnails of past sessions.
- **`open_path`** only opens paths inside the output root, the models folder or the log folder.
- **TLS** uses rustls with the ring provider (installed at startup), matching the updater plugin.

## Test assets

- **Benchmark clip** `src-tauri/resources/bench_ja.wav`: 10 s (4.0–14.0 s) of chapter 4 of the
  LibriVox recording of 新美南吉『ごんぎつね』, released under **CC0 1.0**
  (archive.org item `gongitsune_um_librivox`). 16 kHz mono 16-bit, 320 KB.
- **Integration fixtures** are built by `tests/fixtures/build_fixtures.py` from the same CC0
  recording and the public-domain Aozora Bunko text (card 628) instead of being committed, which
  keeps several megabytes of audio out of the repository. See `tests/fixtures/README.md`.

## Measured results

On the reference machine (RTX 5070 Ti, Vulkan), 2026-10-02. Rechecked on 2026-10-05 before the
first release: the same turbo-q5 CERs, meeting latency p50 509 ms and p95 660 ms.

| Fixture, model | One-pass CER | Streaming CER | Latency (end of utterance → text) |
| --- | --- | --- | --- |
| meeting, turbo-q5, real time | 22.9% | 23.8% | p50 504 ms, p95 639 ms (37 utterances) |
| two-channel, turbo-q5 | — | 相手 23.0%, 自分 23.6% | no echo lines left |
| meeting, base-q5, GPU | 28.9% | 33.1% | — |
| meeting, base-q5, CPU (AVX2, the nightly setup) | 29.1% | 33.1% | p50 810 ms, p95 862 ms |
| two-channel, base-q5, CPU | — | 相手 33.4%, 自分 31.3% | no echo lines left |

turbo-q5 meets section 16 and 18. base-q5 loses about 4 points when cut into utterances, so the
nightly CPU run with base-q5 allows 6 points (`KIKITORI_CER_MARGIN`) and serves as a regression
guard; the three-point rule is checked with turbo-q5 here. Its two-channel 相手 CER sits 1.6
points under the test's 35% limit, so small decoding differences on the runner could trip it.
`KIKITORI_REQUIRE_FIXTURES=1` makes a missing model or fixture fail the run instead of skipping
it.
