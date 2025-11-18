#!/usr/bin/env bash
# Desktop integration installer for Sonori
# Installs .desktop file, AppStream metadata, and icons

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DESKTOP_DIR="$SCRIPT_DIR/desktop"

# Determine installation mode
if [ "$1" = "--system" ]; then
    INSTALL_MODE="system"
    PREFIX="${PREFIX:-/usr}"
    echo "Installing system-wide to $PREFIX"
elif [ "$1" = "--user" ]; then
    INSTALL_MODE="user"
    PREFIX="$HOME/.local"
    echo "Installing for current user to $PREFIX"
else
    echo "Usage: $0 [--system|--user]"
    echo ""
    echo "  --system  Install system-wide (requires root, uses /usr)"
    echo "  --user    Install for current user only (uses ~/.local)"
    echo ""
    echo "You can also set PREFIX environment variable to customize installation location:"
    echo "  PREFIX=/opt ./install-desktop.sh --system"
    exit 1
fi

# Ensure destination directories exist
APPLICATIONS_DIR="$PREFIX/share/applications"
METAINFO_DIR="$PREFIX/share/metainfo"
ICONS_DIR="$PREFIX/share/icons"

echo "Creating directories..."
mkdir -p "$APPLICATIONS_DIR"
mkdir -p "$METAINFO_DIR"
mkdir -p "$ICONS_DIR/hicolor/scalable/apps"

# Install .desktop file
echo "Installing desktop file..."
install -m 644 "$DESKTOP_DIR/sonori.desktop" "$APPLICATIONS_DIR/"

# Install AppStream metadata
echo "Installing AppStream metadata..."
install -m 644 "$DESKTOP_DIR/com.github.0xPD33.sonori.metainfo.xml" "$METAINFO_DIR/"

# Install icons
echo "Installing icons..."
if [ -f "$DESKTOP_DIR/icons/hicolor/scalable/apps/sonori.svg" ]; then
    install -m 644 "$DESKTOP_DIR/icons/hicolor/scalable/apps/sonori.svg" \
        "$ICONS_DIR/hicolor/scalable/apps/"
fi

# Install any PNG icons if they exist
for size_dir in "$DESKTOP_DIR/icons/hicolor/"*"x"*; do
    if [ -d "$size_dir" ]; then
        size=$(basename "$size_dir")
        mkdir -p "$ICONS_DIR/hicolor/$size/apps"
        if [ -f "$size_dir/apps/sonori.png" ]; then
            install -m 644 "$size_dir/apps/sonori.png" \
                "$ICONS_DIR/hicolor/$size/apps/"
        fi
    fi
done

# Update desktop database
echo "Updating desktop database..."
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$APPLICATIONS_DIR" || true
else
    echo "Warning: update-desktop-database not found, skipping database update"
fi

# Update icon cache
echo "Updating icon cache..."
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t "$ICONS_DIR/hicolor" || true
else
    echo "Warning: gtk-update-icon-cache not found, skipping icon cache update"
fi

# Validate AppStream metadata (optional)
if command -v appstreamcli &> /dev/null; then
    echo "Validating AppStream metadata..."
    appstreamcli validate "$METAINFO_DIR/com.github.0xPD33.sonori.metainfo.xml" || true
fi

echo ""
echo "✓ Desktop integration files installed successfully!"
echo ""
echo "The application should now appear in your application menu."
echo "You may need to log out and back in for changes to take full effect."
echo ""

if [ "$INSTALL_MODE" = "user" ]; then
    echo "Note: User installation doesn't require root privileges but only affects your user account."
fi
