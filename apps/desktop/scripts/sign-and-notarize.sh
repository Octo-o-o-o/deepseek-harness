#!/bin/bash
# Deep-sign dshd.app, build a DMG, notarize it, and staple the ticket.
#
# Usage: APP=<path/to/.app> [OUT_DMG=<path>] [IDENTITY=<Developer ID>] \
#        NOTARY_PROFILE=<notarytool keychain profile> ./sign-and-notarize.sh
#
# Every Mach-O inside the payload is signed, not only the files carrying an
# executable bit: Node native addons (`.node`) and the libvips dylibs ship
# without one, and notarization rejects the archive when they are unsigned.
# Inner binaries are signed before the bundle so the outer signature seals a
# tree that is already valid.
set -euo pipefail
APP="${APP:?need APP path to .app}"
OUT_DMG="${OUT_DMG:-$(dirname "$APP")/dshd.dmg}"
IDENTITY="${IDENTITY:-Developer ID Application: Yixiao Wang (5CS6HUB4P2)}"
VOLNAME="${VOLNAME:-dshd}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
ENTITLEMENTS="$WORK/entitlements.plist"
cat > "$ENTITLEMENTS" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
  <key>com.apple.security.cs.disable-library-validation</key><true/>
</dict></plist>
PLIST

echo "== 深度签名（先内层 Mach-O，再 bundle）=="
signed=0
# -perm -111 alone misses .node/.dylib; ask `file` about every regular file.
while IFS= read -r -d '' candidate; do
  if file -b "$candidate" | grep -q 'Mach-O'; then
    codesign --force --options runtime --timestamp \
      --entitlements "$ENTITLEMENTS" --sign "$IDENTITY" "$candidate"
    signed=$((signed + 1))
  fi
done < <(find "$APP" -type f -print0)
echo "已签名内层 Mach-O: $signed"
codesign --force --options runtime --timestamp \
  --entitlements "$ENTITLEMENTS" --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

echo "== 建 DMG =="
rm -f "$OUT_DMG"
STAGE="$WORK/stage"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "$VOLNAME" -srcfolder "$STAGE" -ov -format UDZO "$OUT_DMG"

if [ -z "${NOTARY_PROFILE:-}" ]; then
  echo "== 未公证：NOTARY_PROFILE 未设置 =="
  echo "先执行一次 xcrun notarytool store-credentials <profile>，再重跑本脚本。"
  echo "DMG: $OUT_DMG"
  exit 0
fi

echo "== 公证 =="
# --wait exits non-zero only on submission failure, not on an Invalid verdict,
# so the status is read back and the log is printed before failing.
submit_log="$WORK/submit.json"
xcrun notarytool submit "$OUT_DMG" --keychain-profile "$NOTARY_PROFILE" --wait \
  --output-format json > "$submit_log"
status=$(/usr/bin/plutil -extract status raw -o - "$submit_log" 2>/dev/null || true)
id=$(/usr/bin/plutil -extract id raw -o - "$submit_log" 2>/dev/null || true)
echo "公证结果: ${status:-unknown} (id ${id:-unknown})"
if [ "$status" != "Accepted" ]; then
  echo "== 公证未通过，日志如下 ==" >&2
  [ -n "$id" ] && xcrun notarytool log "$id" --keychain-profile "$NOTARY_PROFILE" >&2 || true
  exit 1
fi

xcrun stapler staple "$OUT_DMG"
xcrun stapler validate "$OUT_DMG"
spctl --assess --type open --context context:primary-signature -v "$OUT_DMG"
echo "DMG（已公证并装订）: $OUT_DMG"
