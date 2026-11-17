#!/bin/sh
# Build dist/RealityHop-mac-<chip>.dmg on a Mac with that chip (apple-silicon or intel).
# Needs: pip install -r requirements.txt -r requirements-mac.txt
# The .dmg holds RealityHop.app (ad-hoc signed, not notarized) and a link to Applications to drag it onto.
set -eu
cd "$(dirname "$0")"
v=$(sed -n "s/^VERSION = '\(.*\)'.*/\1/p" src/version.py)
case "$(uname -m)" in
  arm64) chip=apple-silicon ;;
  x86_64) chip=intel ;;
  *) echo "unknown chip $(uname -m)"; exit 1 ;;
esac
root=$PWD; out=build/mac
rm -rf "$out"; mkdir -p "$out" dist
python -m PyInstaller --noconfirm --windowed --onedir --name RealityHop --icon "$root/src/resources/app.ico" \
  --osx-bundle-identifier com.realityhop.app --add-data "$root/src/resources:resources" --paths "$root/src" \
  --hidden-import macos --hidden-import AppKit --hidden-import Foundation --hidden-import PyObjCTools.AppHelper \
  --exclude-module PIL._avif --exclude-module PIL._webp --exclude-module PIL._imagingft --exclude-module PIL._imagingcms \
  --exclude-module _zstd --exclude-module compression.zstd \
  --distpath "$out/dist" --workpath "$out/work" --specpath "$out" "$root/src/realityhop.pyw"
app="$out/dist/RealityHop.app"; plist="$app/Contents/Info.plist"
set_key() {   # Info.plist: change the key, or add it when PyInstaller did not write it
  /usr/libexec/PlistBuddy -c "Set :$1 $3" "$plist" 2>/dev/null || /usr/libexec/PlistBuddy -c "Add :$1 $2 $3" "$plist"
}
set_key CFBundleDisplayName string "Reality Hop"
set_key CFBundleShortVersionString string "$v"
set_key CFBundleVersion string "$v"
set_key LSMinimumSystemVersion string 15.0
set_key NSHumanReadableCopyright string "(c) 2026 KillerKingSolo, CC BY-NC-ND 4.0"
# a changed Info.plist breaks the signature: sign the whole app again (ad hoc, no Apple ID), then check it
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
stage="$out/dmg"; mkdir -p "$stage"
ditto "$app" "$stage/RealityHop.app"
ln -s /Applications "$stage/Applications"
dmg="dist/RealityHop-mac-$chip.dmg"; rm -f "$dmg" "$out/rw.dmg"
# the disk's own icon (window title, Finder sidebar, desktop): the app icon as .VolumeIcon.icns, plus the volume's
# "has custom icon" Finder flag, set on a writable image that is then compressed
icon=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$plist")
cp "$app/Contents/Resources/${icon%.icns}.icns" "$stage/.VolumeIcon.icns"
hdiutil create -volname "Reality Hop" -srcfolder "$stage" -fs HFS+ -format UDRW -ov "$out/rw.dmg"
mnt=$(mktemp -d)
dev=$(hdiutil attach -nobrowse -noautoopen -mountpoint "$mnt" "$out/rw.dmg" | awk 'NR == 1 { print $1 }')   # /dev/diskN
xattr -wx com.apple.FinderInfo 0000000000000000040000000000000000000000000000000000000000000000 "$mnt"
for i in 1 2 3 4 5 6; do   # Spotlight may still be reading the fresh disk for a moment: "Resource busy"
  [ -e "$dev" ] || break   # the device is gone: done (it may unmount first and detach a moment later)
  hdiutil detach "$dev" && break
  sleep 2; [ "$i" = 6 ] && hdiutil detach -force "$dev"
done
rmdir "$mnt" 2>/dev/null || true
hdiutil convert "$out/rw.dmg" -format ULMO -o "$dmg"   # LZMA: smaller than UDZO (zlib); macOS 10.15 and newer open it
echo "$dmg  $(du -sh "$app" | cut -f1) app, $(du -h "$dmg" | cut -f1) dmg, SHA-256 $(shasum -a 256 "$dmg" | cut -d' ' -f1)"
