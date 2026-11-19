#!/bin/sh
# Build dist/RealityHop-mac-<chip>.dmg from the Rust app on a Mac with that chip (apple-silicon or intel).
# The .dmg holds RealityHop.app (ad hoc signed, not notarized) and a link to Applications to drag it onto.
set -eu
cd "$(dirname "$0")"
v=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
case "$(uname -m)" in
  arm64) chip=apple-silicon ;;
  x86_64) chip=intel ;;
  *) echo "unknown chip $(uname -m)"; exit 1 ;;
esac
cargo build --locked --release -p realityhop-app
out=build/mac; app="$out/RealityHop.app"
rm -rf "$out"; mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" dist
cp target/release/RealityHop "$app/Contents/MacOS/RealityHop"
# the icon: app/icons/icon.png at every size macOS asks for
set_dir="$out/app.iconset"; mkdir -p "$set_dir"
for s in 16 32 128 256 512; do
  sips -z $s $s app/icons/icon.png --out "$set_dir/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); sips -z $d $d app/icons/icon.png --out "$set_dir/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$app/Contents/Resources/app.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>RealityHop</string>
  <key>CFBundleIdentifier</key><string>com.realityhop.app</string>
  <key>CFBundleName</key><string>RealityHop</string>
  <key>CFBundleDisplayName</key><string>Reality Hop</string>
  <key>CFBundleIconFile</key><string>app.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$v</string>
  <key>CFBundleVersion</key><string>$v</string>
  <key>LSMinimumSystemVersion</key><string>15.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>(c) 2026 KillerKingSolo, CC BY-NC-ND 4.0</string>
</dict></plist>
EOF
codesign --force --deep --sign - "$app"   # ad hoc, no Apple ID
codesign --verify --deep --strict "$app"
stage="$out/dmg"; mkdir -p "$stage"
ditto "$app" "$stage/RealityHop.app"
ln -s /Applications "$stage/Applications"
cp "$app/Contents/Resources/app.icns" "$stage/.VolumeIcon.icns"
dmg="dist/RealityHop-mac-$chip.dmg"; rm -f "$dmg" "$out/rw.dmg"
hdiutil create -volname "Reality Hop" -srcfolder "$stage" -fs HFS+ -format UDRW -ov "$out/rw.dmg"
mnt=$(mktemp -d)
dev=$(hdiutil attach -nobrowse -noautoopen -mountpoint "$mnt" "$out/rw.dmg" | awk 'NR == 1 { print $1 }')
xattr -wx com.apple.FinderInfo 0000000000000000040000000000000000000000000000000000000000000000 "$mnt"   # the disk shows its icon
for i in 1 2 3 4 5 6; do   # Spotlight may still be reading the fresh disk for a moment
  [ -e "$dev" ] || break
  hdiutil detach "$dev" && break
  sleep 2; [ "$i" = 6 ] && hdiutil detach -force "$dev"
done
rmdir "$mnt" 2>/dev/null || true
hdiutil convert "$out/rw.dmg" -format ULMO -o "$dmg"
echo "$dmg  $(du -sh "$app" | cut -f1) app, $(du -h "$dmg" | cut -f1) dmg, SHA-256 $(shasum -a 256 "$dmg" | cut -d' ' -f1)"
