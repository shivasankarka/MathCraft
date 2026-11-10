#!/usr/bin/env bash
# Wrap the mathcraft binary in a macOS application bundle with the app icon.
# usage: scripts/macos-app.sh <binary> <version> <output-dir>
# Writes <output-dir>/MathCraft.app (unsigned: macOS asks for confirmation the first time it is opened).
set -euo pipefail

bin="$1"
version="${2#v}"
out="$3"
app="$out/MathCraft.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/mathcraft"
cp "$(dirname "$0")/../assets/icon/mathcraft.icns" "$app/Contents/Resources/mathcraft.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>MathCraft</string>
    <key>CFBundleDisplayName</key><string>MathCraft</string>
    <key>CFBundleIdentifier</key><string>org.mathcraft.app</string>
    <key>CFBundleExecutable</key><string>mathcraft</string>
    <key>CFBundleIconFile</key><string>mathcraft</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
echo "$app"
