# Changelog

All notable changes are listed here. Versions follow [SemVer](https://semver.org/); `src-tauri/tauri.conf.json` holds the current version.

## [Unreleased]

### Added

- A cut button while recording starts a new part of the session. The transcript shows the cut, Markdown exports head each part with `## HH:MM:SS`, and the PDF lists the parts in its outline.

### Changed

- 最新へ jumps straight to the newest line when it is far away, instead of gliding there for seconds in a long session.

## [0.1.0] - 2026-10-05

### Added

- Live transcription with local Whisper (whisper.cpp), on the GPU through Vulkan with CPU fallback. Japanese and English come out as spoken, whichever language is set, never translated.
- Sources: one app's audio (Windows 11), all system audio, and the microphone, labelled 相手 and 自分, with an echo guard for speakers. The source picker shows each app's live sound beside its name while it plays.
- Screenshots by hotkey or button, placed in the transcript where they were taken.
- Sessions saved as Markdown with images, recovered after a crash, and kept in a searchable history grouped by day, with line editing and moving several to the Recycle Bin at once.
- A small window in white, black and one red that follows the Windows light or dark theme, with a pin to keep it on top. A red dot starts a line drawn from the live sound, 相手 above and 自分 below, under a transcript grouped into speaker turns, with pause/resume, stop and screenshot controls. Each finished session keeps the shape of its sound as a timeline to jump through and as its picture in the history.
- Copy as plain text, Markdown or a prompt for AI agents; export, each saved where you choose, to Markdown (a folder or a ZIP with the images), PDF typeset with Typst in Yu Gothic, and the Typst file itself to edit.
- Model manager with resumable, checksum-verified downloads and a 10-second speed benchmark.
- Setup wizard, tray menu, global hotkeys (off until you set them), Japanese and English UI; the first launch follows the Windows display language.
- App icon and logo: a red bird (kiki-tori; とり is a bird) whose outline ripples out in fading waves. In the tray the ripples show only while recording (grey while paused or finishing); the title bar shows the kikitori logo, with the bird as its o, and the PDF footer carries the bird.
- Daily update check that asks before installing (active once updater keys are set up).
- NSIS installer that installs per user without admin rights and bundles the Vulkan loader.
