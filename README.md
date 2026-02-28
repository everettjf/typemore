# Typemore

<p align="center">
  Offline speech recognition desktop app built with Tauri + React + Rust.
</p>

<p align="center">
  <a href="https://discord.gg/YOUR_DISCORD_INVITE"><img alt="Discord" src="https://img.shields.io/badge/Discord-Join%20Community-5865F2?logo=discord&logoColor=white"></a>
  <a href="https://your-website.example.com"><img alt="Website" src="https://img.shields.io/badge/Website-Coming%20Soon-0EA5E9"></a>
  <img alt="License" src="https://img.shields.io/badge/License-TBD-lightgrey">
</p>

## Table of Contents

- [Screenshot](#screenshot)
- [Overview](#overview)
- [Features](#features)
- [Tech Stack](#tech-stack)
- [Project Structure](#project-structure)
- [Requirements](#requirements)
- [Quick Start](#quick-start)
- [Build and Validation](#build-and-validation)
- [Data Storage](#data-storage)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)
- [Star History](#star-history)

## Screenshot

> Place your screenshot here: `docs/images/screenshot-main.png`

![Typemore Screenshot](docs/images/screenshot-main.png)

## Overview

Typemore is a desktop app focused on local/offline ASR (Automatic Speech Recognition).  
It is designed for fast speech capture, transcript generation, and recording management on macOS.

- Local-first recognition workflow
- Modern desktop UX with Tauri
- Rust backend command pipeline
- React frontend for recording/transcript interaction

## Features

- Recording list on the left, transcript editor panel on the right
- One-click model initialization (download + extract)
- Microphone recording (click to start, click again to stop)
- Automatic transcription after recording stops
- Re-transcribe from selected historical recordings
- Context menu actions for rename/delete recordings

## Tech Stack

- **Desktop Runtime:** Tauri v2
- **Frontend:** React 19 + TypeScript + Vite
- **Backend:** Rust
- **ASR Engine:** sherpa-rs / sherpa-onnx
- **UI Libraries:** Radix UI, Tailwind CSS, Chart.js

## Project Structure

```text
.
├── src/                 # React frontend
├── public/              # Static assets
├── src-tauri/           # Rust backend + Tauri config
│   ├── src/
│   └── tauri.conf.json
├── docs/
│   └── images/          # README screenshots (recommended)
└── README.md
```

## Requirements

- macOS (recommended for current setup)
- Bun 1.x
- Rust stable toolchain
- Tauri dependencies for your platform

## Quick Start

```bash
bun install
bun run tauri dev
```

On first launch, click **Initialize Model** in the app to download:

- `sherpa-onnx-paraformer-trilingual-zh-cantonese-en`
- Source: `k2-fsa/sherpa-onnx` GitHub Releases (`asr-models`)

## Build and Validation

```bash
# Frontend type-check + production build
bun run build

# Rust compile check
cd src-tauri && cargo check
```

## Release to Homebrew

Prepare your release archive in `build/` (for example `build/typemore-v0.1.1-macos.zip`), then run:

```bash
TAP_REPO=yourname/homebrew-tap \
FORMULA_PATH=Casks/typemore.rb \
./scripts/release_to_homebrew.sh
```

Notes:

- `TAP_REPO` is required, format: `owner/repo`.
- `FORMULA_PATH` is required, e.g. `Casks/typemore.rb` or `Formula/typemore.rb`.
- If your file is not under `build/`, provide `ASSET_PATH=/absolute/path/to/file.zip`.
- To skip auto version bump: `SKIP_BUMP=1`.
- A cask template is provided at `docs/homebrew/typemore.rb.example`.

## Data Storage

The app stores model and audio files under Tauri `app_data_dir`:

- `sherpa-model/`
- `recordings/`

## Roadmap

- Better model management (multi-model switch)
- Batch transcription workflows
- Export transcript to Markdown/TXT
- Speaker diarization and timeline view

## Contributing

Contributions are welcome.

1. Fork this repo
2. Create a feature branch: `git checkout -b feat/your-feature`
3. Commit your changes
4. Open a Pull Request

For larger feature proposals, open an issue first for discussion.

## License

License is currently TBD. Add a `LICENSE` file before public release.

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=everettjf/Typemore&type=Date)](https://www.star-history.com/#everettjf/Typemore&Date)
