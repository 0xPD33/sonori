# Release Process

Sonori uses `cargo-release` to automate version bumping, tagging, and pushing releases.

## Prerequisites

Install cargo-release:
```bash
cargo install cargo-release
```

## Making a Release

1. **Ensure everything is committed:**
   ```bash
   git status  # Should be clean
   ```

2. **Run cargo-release:**
   ```bash
   # Patch release (0.3.0 -> 0.3.1)
   cargo release patch --execute

   # Minor release (0.3.0 -> 0.4.0)
   cargo release minor --execute

   # Major release (0.3.0 -> 1.0.0)
   cargo release major --execute
   ```

3. **That's it!** cargo-release will:
   - Bump version in Cargo.toml
   - Update Cargo.lock
   - Create a git commit
   - Create a git tag (v0.4.0)
   - Push to GitHub
   - GitHub Actions automatically builds and creates the release

## What Happens Next

When the tag is pushed to GitHub, the `.github/workflows/release.yml` workflow automatically:
- Builds the release binary
- Runs tests
- Creates a GitHub release with artifacts
- Generates release notes

## Configuration

Release settings are in `Cargo.toml`:

```toml
[package.metadata.release]
publish = false          # Don't publish to crates.io
push = true             # Push automatically
sign-tag = false        # No GPG signing
tag-message = "Release {{version}}"
pre-release-commit-message = "chore: Release sonori version {{version}}"
```

## Preview Mode

To see what will happen without making changes:
```bash
cargo release minor  # Without --execute flag
```
