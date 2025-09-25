# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` bootstraps the overlay, while `src/lib.rs` exposes shared systems used by both CLI and UI paths.
- Domain modules such as `src/audio_capture.rs`, `src/transcription_processor.rs`, and `src/silero_audio_processor.rs` handle audio IO and inference orchestration; cross-cutting aliases live in `src/prelude.rs`.
- UI components and shaders live under `src/ui/` (with `.wgsl` GPU pipelines); reusable art sits in `assets/`, and `sounds/` contains audio prompts and cues.
- `config.toml` in the repo root mirrors runtime defaults; user-local overrides are resolved alongside the binary at launch.
- Support tooling resides in `model-conversion/` (Nix shell plus scripts) and reference docs such as `ARCHITECTURE.md`—skim them before refactoring core flows.

## Build, Test, and Development Commands
- `nix develop` drops you into a shell with Vulkan, PortAudio, and other dependencies aligned with CI expectations.
- `cargo build --release` produces the optimized overlay binary in `target/release/sonori`.
- `cargo run -- --cli` launches transcription in headless mode; omit `--cli` to exercise the on-screen overlay.
- `cargo fmt --all` and `cargo clippy --all-targets --all-features` enforce formatting and linting before review.

## Coding Style & Naming Conventions
- Follow `rustfmt` defaults (4-space indent, trailing commas) and keep modules, files, and functions snake_case; types stay in UpperCamelCase.
- Prefer explicit `use crate::...` imports for clarity; consolidate shared aliases in `src/prelude.rs`.
- When adding shaders or UI assets, match existing names (`text_window.wgsl`, `spectogram.rs`) and keep asset filenames lowercase with dashes only when necessary.

## Testing Guidelines
- Co-locate unit tests inside modules under `#[cfg(test)]`; integration tests belong in `tests/` should you add broader coverage.
- Run `cargo test --all` before pushing; add targeted runs (e.g., `cargo test silero_audio_processor`) when touching audio or GPU pipelines.
- Document any manual verification (overlay renders on Wayland, transcripts stream to `transcription_stats.log`) in your PR notes.

## Commit & Pull Request Guidelines
- Commit titles should stay under ~60 characters, use imperative mood, and highlight the user-visible change (see `bump version number`, `add manual transcription mode`).
- Squash incidental WIP commits locally; each PR should describe scope, call out platforms tested, and link related issues or TODO items.
- For UI-affecting updates, attach screenshots or brief screen capture notes; for CLI paths, include sample command invocations.
- Include a concise verification checklist covering build, test, and runtime sanity so reviewers can reproduce.
