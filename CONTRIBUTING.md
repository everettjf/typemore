# Contributing

Thanks for contributing to TypeMore.

## Stack

- Tauri v2
- React 19 + TypeScript + Vite
- Rust
- sherpa-rs / sherpa-onnx
- Radix UI + Tailwind CSS

## Project Structure

```text
.
├── src/                    # React frontend
├── src-tauri/              # Rust backend and Tauri config
├── icons/                  # Source app icons and branding assets
├── scripts/                # Build and release helpers
├── website/                # Landing page
└── deploy.sh               # Release automation
```

## Local Development

Install dependencies:

```bash
bun install
```

Run the desktop app in development:

```bash
bun run tauri dev
```

Run only the frontend:

```bash
bun run dev
```

## Build And Verify

Frontend:

```bash
bun run build
```

Rust backend:

```bash
cd src-tauri && cargo check
```

Build the DMG locally:

```bash
./scripts/build_dmg.sh
```

## Release

`deploy.sh` is the canonical release script. It can:

- bump the patch version automatically
- build the signed macOS bundle
- notarize and staple the DMG
- create or update the GitHub release
- update the Homebrew cask in `everettjf/homebrew-tap`

Standard release:

```bash
./deploy.sh
```

Release the current version without bumping:

```bash
SKIP_BUMP=1 ./deploy.sh
```
