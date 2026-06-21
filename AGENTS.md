# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` bootstraps the overlay, while `src/lib.rs` exposes shared systems used by both CLI and UI paths.
- **Speech Runtime**: reusable STT infrastructure lives in the sibling `../speechcore` crate. Sonori imports `speechcore` for audio capture, VAD, transcription orchestration, backend abstraction, model downloads, transcript streams, and stats.
- **Sonori App Layer**: app-local code owns the overlay UI, CLI/IPC entrypoints, config conversion, portal integration, system tray, sound playback, transcript history, and Magic Mode enhancement.
- **Sound System**: `src/sound_player.rs` (CPAL-based playback) and `src/sound_generator.rs` (tone synthesis) handle audio feedback for UI state transitions.
- **System Integration**: `src/portal_input.rs`, `src/portal_tokens.rs`, `src/global_shortcuts.rs`, and `src/system_tray.rs` handle XDG Portal integration and system tray presence.
- **UI Components**: `src/ui/` directory contains rendering pipeline, text rendering, spectrogram visualization, and button system; `src/ui/*.wgsl` contains custom GPU shaders.
- **Configuration**: `config.toml` in repo root mirrors runtime defaults with hierarchical sections for general, backend, audio, VAD, sound, portal, window, and manual mode settings.
- **Documentation & Tooling**: `model-conversion/` for model utilities, `ARCHITECTURE.md` for system design, `CLAUDE.md` for dev guidelines, `AGENTS.md` for repo conventions.

## Build, Test, and Development Commands

### Prerequisites by Platform
- **NixOS**: `nix develop` provides all dependencies (Vulkan, CPAL, portaudio, OpenBLAS, shaderc, vulkan-headers, etc.)
- **Ubuntu/Debian**: Requires `libvulkan-dev vulkan-headers libopenblas-dev shaderc` plus audio libs
- **Fedora/RHEL**: Requires `vulkan-loader-devel vulkan-headers openblas-devel shaderc` plus audio libs
- **Arch**: `vulkan-headers blas shaderc` plus portaudio

### Build Commands
- `nix develop` drops you into a shell with all dependencies aligned with CI expectations.
- `cargo build --release` produces the optimized binary in `target/release/sonori`.
- `cargo run -- --cli` launches in headless mode; omit `--cli` for GUI overlay.
- Backend feature builds can be checked with commands like `cargo check --no-default-features --features backend-whisper-cpp`.

### GPU & Backend-Specific Builds
- Sonori forwards backend features to `speechcore`: `backend-whisper-cpp`, `backend-ctranslate2`, `backend-moonshine`, and `backend-parakeet`.
- **Whisper.cpp with Vulkan**: Requires `shaderc` and `vulkan-headers` for shader compilation.
- **Whisper.cpp with OpenBLAS**: Requires `libopenblas-dev` or equivalent.
- **CTranslate2 with CUDA**: Requires CUDA toolkit (not currently tested/documented).

- `cargo fmt --all` and `cargo clippy --all-targets --all-features` enforce formatting and linting before review.

## Coding Style & Naming Conventions
- Follow `rustfmt` defaults (4-space indent, trailing commas) and keep modules, files, and functions snake_case; types stay in UpperCamelCase.
- Prefer explicit app-local imports for Sonori systems and direct `speechcore::...` imports for shared STT types.
- When adding shaders or UI assets, match existing names (`text_window.wgsl`, `spectogram.rs`) and keep asset filenames lowercase with dashes only when necessary.

## Testing Guidelines
- Co-locate unit tests inside modules under `#[cfg(test)]`; integration tests belong in `tests/` should you add broader coverage.
- Run `cargo test --all` before pushing. When touching shared STT behavior, also run focused checks in `../speechcore`.
- Document any manual verification (overlay renders on Wayland, transcripts stream to `transcription_stats.log`) in your PR notes.

## Commit & Pull Request Guidelines
- Commit titles should stay under ~60 characters, use imperative mood, and highlight the user-visible change (see `bump version number`, `add manual transcription mode`).
- Squash incidental WIP commits locally; each PR should describe scope, call out platforms tested, and link related issues or TODO items.
- For UI-affecting updates, attach screenshots or brief screen capture notes; for CLI paths, include sample command invocations.
- Include a concise verification checklist covering build, test, and runtime sanity so reviewers can reproduce.
