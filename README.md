# TypeMore

[中文说明](./README.zh-CN.md)

TypeMore is a macOS desktop app for offline speech-to-text. It captures your voice locally, runs speech recognition on-device, and pastes the result back into the active input with a low-friction hotkey workflow.

![TypeMore interface](./website/public/images/app-mockup.jpg)

## Why TypeMore

- Offline first: audio stays on your Mac.
- Native desktop workflow: global hotkeys, overlay feedback, recording history.
- Open source: built with Tauri + React + Rust.
- Practical for writing: dictation, cleanup, optional cloud post-processing, translation.

## How It Works

TypeMore uses a local speech recognition pipeline based on `sherpa-onnx` through `sherpa-rs`.

1. On first launch, TypeMore downloads the speech model into the app data directory.
2. When you hold or tap the configured hotkey, the app records microphone audio locally.
3. The Rust backend converts audio to 16k mono WAV and runs offline ASR.
4. The recognized text is shown in the app, cached with the recording, and can be pasted back into your current input target.
5. If you enable cloud post-processing, the local transcript can be cleaned up or translated by your configured provider after local ASR finishes.

This design keeps the critical path local. Cloud providers are optional and never required for basic dictation.

## Install

### Homebrew

```bash
brew update && brew install --cask everettjf/tap/typemore
```

Upgrade later with:

```bash
brew upgrade --cask typemore
```

### Direct Download

Download the latest notarized DMG from GitHub Releases:

- Releases page: <https://github.com/everettjf/typemore/releases>
- Latest DMG: <https://github.com/everettjf/typemore/releases/latest/download/TypeMore.dmg>

## Community

- Website: <https://typemore.app>
- Discord: <https://discord.com/invite/eGzEaP6TzR>

## Features

- Offline speech recognition on macOS
- Built-in `Fn` / `Fn+Shift` trigger flow
- Global custom hotkeys
- Recording history with rename, delete, and re-transcribe
- Dictionary support for custom words
- Optional cloud optimization and translation
- Startup update check with 7-day reminder

## Data Storage

TypeMore stores runtime data under the Tauri app data directory, including:

- downloaded ASR model files
- recordings
- transcript cache
- dictionary words
- temporary conversion files

## Contributing

If you want to build, debug, or release TypeMore locally, see [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

Apache-2.0. See [LICENSE](./LICENSE).
