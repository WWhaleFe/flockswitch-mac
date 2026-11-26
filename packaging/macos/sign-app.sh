#!/bin/bash
# Signs flockswitch2.app with one fixed identity, so macOS keeps the Accessibility and
# Input Monitoring grants across updates (an ad-hoc signature changes with every build
# and the old grant stops matching). Identity: $FLOCK_SIGN_ID, else the keychain
# identity "flockswitch2 Self Signed" (key backup: AppDev/flockswitch/mac-signing/).
set -euo pipefail
app="$1"
id="${FLOCK_SIGN_ID:-}"
if [[ -z "$id" ]]; then
  id=$(security find-identity -p codesigning 2>/dev/null | awk -F'"' '/"flockswitch2 Self Signed"/ { split($1, a, " "); print a[2]; exit }')
fi
if [[ -n "$id" ]]; then
  codesign --force --sign "$id" --identifier io.github.wwhalefe.flockswitch2 "$app"
else
  echo "WARNING: no 'flockswitch2 Self Signed' identity; signing ad-hoc. Users will have to grant permissions again after this update." >&2
  codesign --force --sign - "$app"
fi
