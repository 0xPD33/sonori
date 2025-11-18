# Sonori Icons

This directory contains placeholder icons for the Sonori application. The current SVG is a simple placeholder design.

## Icon Sizes

The hicolor theme specification supports the following icon sizes for applications:

- **Scalable** (SVG): `hicolor/scalable/apps/sonori.svg` (already provided)
- **PNG sizes** (recommended to generate from your final SVG):
  - 16x16
  - 22x22
  - 24x24
  - 32x32
  - 48x48
  - 64x64
  - 128x128
  - 256x256
  - 512x512

## Creating Your Own Icon

Replace `hicolor/scalable/apps/sonori.svg` with your own design. Consider:

1. **Design elements**: Should represent speech/transcription/audio
2. **Colors**: Use your brand colors
3. **Simplicity**: Icons should be recognizable at small sizes
4. **File format**: SVG is preferred for scalability

## Generating PNG Icons

Once you have your final SVG design, generate PNG versions:

```bash
# Using Inkscape (if available)
for size in 16 22 24 32 48 64 128 256 512; do
  inkscape -w $size -h $size \
    hicolor/scalable/apps/sonori.svg \
    -o hicolor/${size}x${size}/apps/sonori.png
done

# Or using ImageMagick/convert
for size in 16 22 24 32 48 64 128 256 512; do
  convert -background none \
    -resize ${size}x${size} \
    hicolor/scalable/apps/sonori.svg \
    hicolor/${size}x${size}/apps/sonori.png
done
```

## Installation

Icons are installed by the `install-desktop.sh` script to:
- System-wide: `/usr/share/icons/hicolor/`
- User-specific: `~/.local/share/icons/hicolor/`

After installation, run `gtk-update-icon-cache` to refresh the icon cache.
