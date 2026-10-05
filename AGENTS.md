# AGENTS.md

Kikitori is a Windows desktop app that transcribes live audio (one app, the whole system, or the
mic) with local Whisper, drops screenshots into the transcript where they were taken, and exports
Markdown, PDF and Typst. Tauri 2 + React + TypeScript in `src/`, Rust in `src-tauri/`, whisper.cpp
through `whisper-rs`, WASAPI capture.

## Read first

- `AppSpec.md` is the spec. Code comments cite it ("section 8", "FR-53"); read the section for
  what you are changing.
- `docs/DECISIONS.md` holds every choice the spec leaves open and every deviation from it. Where
  the spec is silent, pick the simplest option that keeps the UI one-tap simple and add a line
  there. When you change something the spec defines, update the spec section too.
- The look (white paper, ink, one red; "one dot, one line") is deliberate. Read
  `docs/DECISIONS.md` → UI before changing the UI.

## Commands

Windows 10/11 x64 with the MSVC build tools, Rust (the version in `.github/workflows/ci.yml`),
Node LTS and pnpm (`corepack enable`). Install with pnpm, never `npm install` (the lockfile is
`pnpm-lock.yaml`); `npm run <script>` is fine for running scripts. GPU builds also need the
Vulkan SDK and LLVM (see `README.en.md` → For developers).

```sh
pnpm install
pnpm tauri dev    # CPU build
pnpm dev:gpu      # Vulkan build
pnpm build:gpu    # the release installer, as CI builds it
```

Vulkan builds need a short cargo target dir, or whisper.cpp's shader build passes Windows'
260-character path limit: `dev:gpu` and `build:gpu` use `%USERPROFILE%\.kt`. Set
`CARGO_TARGET_DIR` to it for any cargo command with `--features gpu-vulkan`.

Before you finish, run what CI runs:

```sh
pnpm lint && pnpm typecheck && pnpm test && pnpm build
cd src-tauri
cargo fmt --check
cargo clippy --all-targets --features gpu-vulkan -- -D warnings
cargo test
```

The streaming accuracy tests in `src-tauri/tests/pipeline.rs` are `#[ignore]`d; its header and
`tests/fixtures/README.md` explain how to build the fixtures and run them. Partial (grey) text
only runs on the GPU, so its tests need `--features gpu-vulkan`.

## Conventions

- Code, comments, commit messages and docs are in English. UI text is Japanese first, English
  second: every string goes through i18next with keys in both `src/i18n/ja.json` and
  `src/i18n/en.json` (a test checks they match).
- The README is the exception: `README.md` is Japanese and `README.en.md` is its English twin.
  Change both together; the developer sections live only in the English one.
- Nothing the user records leaves the PC. The only network use is model downloads and the daily
  update check; keep it that way.
- Rust is the source of truth for IPC. The TypeScript in `src/ipc/` is written by hand to match,
  with camelCase fields. A new command also needs its name in `src-tauri/build.rs` and
  `allow-<command>` in `src-tauri/capabilities/default.json`.
- OS-specific code stays behind the traits in spec section 6 (`AudioSource`, `ScreenCapturer`,
  `AudioAppLister`), so a macOS port only adds files.
- Library APIs drift: read the current docs before using a crate or plugin. Drift-prone crates
  are pinned with `=`; both lockfiles are committed.
- Never invent model URLs, SHA-256 hashes or file sizes; take them from the source.
- rustfmt uses `max_width = 120`. Files are LF (`.gitattributes`).
- Exports are snapshot-tested with `insta` (`src-tauri/src/export/snapshots/`). Check a changed
  snapshot before accepting it.
- Comments say why, not what. Match the code around you.
- A commit's subject is one imperative line saying what changed for the user, for example
  "Show apps' live sound, keep one title bar, and draw the line less often".
- Never commit updater signing keys, Whisper models or generated fixtures (all git-ignored).
