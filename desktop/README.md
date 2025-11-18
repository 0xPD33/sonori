# Desktop Integration for Sonori

This directory contains files for integrating Sonori with your Linux desktop environment.

## Contents

- **sonori.desktop** - Desktop entry file for application menus (KDE, GNOME, etc.)
- **com.github.0xPD33.sonori.metainfo.xml** - AppStream metadata for software centers (Discover, GNOME Software)
- **icons/** - Application icons in hicolor theme hierarchy

## Installation

### Quick Install

**For NixOS users:**
Desktop integration is automatically handled by the Nix flake. No manual installation needed.

**For other distributions:**

Install for current user only (recommended):
```bash
./install-desktop.sh --user
```

Install system-wide (requires root):
```bash
sudo ./install-desktop.sh --system
```

### Manual Installation

If you prefer manual installation or need to customize paths:

#### User Installation (~/.local)

```bash
# Install desktop file
mkdir -p ~/.local/share/applications
install -m 644 desktop/sonori.desktop ~/.local/share/applications/

# Install AppStream metadata
mkdir -p ~/.local/share/metainfo
install -m 644 desktop/com.github.0xPD33.sonori.metainfo.xml ~/.local/share/metainfo/

# Install icon
mkdir -p ~/.local/share/icons/hicolor/scalable/apps
install -m 644 desktop/icons/hicolor/scalable/apps/sonori.svg ~/.local/share/icons/hicolor/scalable/apps/

# Update caches
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
```

#### System Installation (/usr/share)

```bash
# Install desktop file
sudo install -m 644 desktop/sonori.desktop /usr/share/applications/

# Install AppStream metadata
sudo install -m 644 desktop/com.github.0xPD33.sonori.metainfo.xml /usr/share/metainfo/

# Install icon
sudo mkdir -p /usr/share/icons/hicolor/scalable/apps
sudo install -m 644 desktop/icons/hicolor/scalable/apps/sonori.svg /usr/share/icons/hicolor/scalable/apps/

# Update caches
sudo update-desktop-database /usr/share/applications
sudo gtk-update-icon-cache -f -t /usr/share/icons/hicolor
```

## Customizing the Icon

The included SVG icon is a placeholder. To create your own:

1. Replace `icons/hicolor/scalable/apps/sonori.svg` with your design
2. Optionally generate PNG versions for different sizes (see `icons/README.md`)
3. Reinstall using the install script or manual commands above

## Desktop Entry Actions

The desktop file includes quick actions accessible from the application menu:

- **Run in CLI Mode** - Start Sonori in terminal mode
- **Run in Manual Mode** - Start with manual transcription mode enabled

In KDE, right-click the Sonori icon in the application menu to see these actions.

## AppStream Metadata

The AppStream metadata file enables:

- Rich application information in software centers
- Screenshots and feature descriptions
- Version history and release notes
- Categories and keywords for better discoverability

### Adding Screenshots

To add screenshots to the AppStream metadata:

1. Take screenshots of Sonori in action
2. Upload them to your repository (e.g., `docs/screenshots/`)
3. Edit `com.github.0xPD33.sonori.metainfo.xml` and uncomment/update the `<image>` tags

## Verifying Installation

After installation, verify everything works:

```bash
# Check desktop file is recognized
desktop-file-validate ~/.local/share/applications/sonori.desktop

# Check AppStream metadata
appstreamcli validate ~/.local/share/metainfo/com.github.0xPD33.sonori.metainfo.xml

# Search for Sonori in your application menu
# It should appear under Audio/Video or Utilities categories
```

## Uninstallation

**User installation:**
```bash
rm ~/.local/share/applications/sonori.desktop
rm ~/.local/share/metainfo/com.github.0xPD33.sonori.metainfo.xml
rm ~/.local/share/icons/hicolor/scalable/apps/sonori.svg
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
```

**System installation:**
```bash
sudo rm /usr/share/applications/sonori.desktop
sudo rm /usr/share/metainfo/com.github.0xPD33.sonori.metainfo.xml
sudo rm /usr/share/icons/hicolor/scalable/apps/sonori.svg
sudo update-desktop-database /usr/share/applications
sudo gtk-update-icon-cache -f -t /usr/share/icons/hicolor
```

## Troubleshooting

**Application doesn't appear in menu:**
- Log out and back in
- Run `update-desktop-database` on the appropriate directory
- Check that the `sonori` binary is in your PATH

**Icon doesn't show:**
- Run `gtk-update-icon-cache` on the hicolor directory
- Check icon file permissions (should be 644)
- Some desktop environments may need a restart

**Desktop file validation errors:**
```bash
desktop-file-validate desktop/sonori.desktop
```

## For Package Maintainers

If you're creating a distribution package (DEB, RPM, AUR, etc.):

1. Install desktop files to appropriate locations:
   - Desktop entry: `$PREFIX/share/applications/`
   - AppStream metadata: `$PREFIX/share/metainfo/`
   - Icons: `$PREFIX/share/icons/hicolor/`

2. In post-install scripts, update caches:
   ```bash
   update-desktop-database -q
   gtk-update-icon-cache -q -t -f $PREFIX/share/icons/hicolor
   ```

3. Consider adding these as runtime dependencies:
   - `desktop-file-utils` (for desktop database)
   - `gtk-update-icon-cache` or `gtk+3` (for icon cache)
   - `appstream` (for AppStream validation)

## Standards Compliance

- **Desktop Entry**: Follows [FreeDesktop.org Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry-spec/latest/)
- **AppStream**: Compliant with [AppStream Specification v1.0](https://www.freedesktop.org/software/appstream/docs/)
- **Icon Theme**: Follows [Icon Theme Specification](https://specifications.freedesktop.org/icon-theme-spec/latest/)
