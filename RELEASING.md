# Release Process

Sonori uses `cargo-release` to automate version bumping, tagging, and pushing releases.

## Prerequisites

Install required tools:
```bash
cargo install cargo-release
cargo install git-cliff  # For automatic changelog generation
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
- Generates a changelog from commits using git-cliff
- Creates a GitHub release with artifacts and formatted release notes

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

## Changelog Management

The project uses **git-cliff** to automatically generate changelogs from conventional commits. This ensures consistent, well-formatted release notes without manual maintenance.

### How It Works

1. **Locally**: You can generate CHANGELOG.md anytime to preview changes
2. **GitHub Actions**: Automatically generates changelog for each release
3. **Conventional Commits**: Parses commit messages to categorize changes
4. **Configuration**: Uses `cliff.toml` to define formatting and grouping rules

### Using git-cliff Locally

**Update CHANGELOG.md:**
```bash
git-cliff -o CHANGELOG.md
```

**Preview upcoming release changes:**
```bash
git-cliff --unreleased
```

**View changelog for specific range:**
```bash
git-cliff v0.4.0..HEAD
```

**View changes without CHANGELOG header:**
```bash
git-cliff --strip all v0.4.1..v0.4.2
```

**Test what the workflow will generate:**
```bash
# Simulate the GitHub release notes
PREV_TAG=$(git describe --tags --abbrev=0 HEAD^)
git-cliff --strip all ${PREV_TAG}..HEAD
```

### Commit Message Convention

For proper changelog generation, follow the [Conventional Commits](https://www.conventionalcommits.org/) specification:

**Format:** `<type>: <description>`

**Supported types:**
- `feat:` - New features (→ **Features** section)
- `fix:` - Bug fixes (→ **Bug Fixes** section)
- `docs:` - Documentation (→ **Documentation** section)
- `perf:` - Performance improvements (→ **Performance** section)
- `refactor:` - Code refactoring (→ **Refactoring** section)
- `style:` - Code style changes (→ **Styling** section)
- `test:` - Test changes (→ **Testing** section)
- `chore:` - Maintenance tasks (→ **Miscellaneous Tasks** section)
- Any commit with `security` in body (→ **Security** section)

**Examples:**
```bash
git commit -m "feat: add CTranslate2 backend support"
git commit -m "fix: resolve protobuf ABI conflict in Nix build"
git commit -m "docs: update installation instructions"
git commit -m "perf: optimize audio buffer processing"
```

**Notes:**
- `chore(release):` commits are automatically skipped from changelogs
- Commit messages that don't follow the convention are filtered out
- The first line of the commit message is used (multiline commits show only first line)

### Changelog Configuration

The `cliff.toml` file controls how changelogs are generated:

```toml
# Categorization rules
commit_parsers = [
  { message = "^feat", group = "Features"},
  { message = "^fix", group = "Bug Fixes"},
  # ... etc
]
```

You can modify `cliff.toml` to:
- Change section names/order
- Add custom commit types
- Modify the changelog format
- Filter specific commits

See [git-cliff documentation](https://git-cliff.org) for all options.

### Integration with GitHub Releases

When you push a tag, the workflow:
1. Installs git-cliff
2. Finds the previous tag
3. Generates changelog for commits between tags
4. Combines it with installation instructions
5. Creates a formatted GitHub release

**Example release notes output:**
```markdown
## Sonori v0.4.2

### Installation

**NixOS (Recommended):**
\`\`\`bash
nix run github:0xPD33/sonori
\`\`\`

### Changes

### Bug Fixes
- unify protobuf across C++ dependencies to resolve segfault

### Features
- add automatic changelog generation
```
