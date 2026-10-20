#!/usr/bin/env bash
# Check that a trunk build of the web app is the demo: its JavaScript glue imports none of the
# browser calls a download needs (an object URL for the file, a link's `download`). The
# wasm can only call what the glue imports, so a demo without them has no way to hand a file
# out, whatever is patched in the wasm.
#
#   scripts/check-demo-build.sh DEMO_DIST [FULL_DIST]
#
# With FULL_DIST (a build without the demo feature), also check that the full build does
# import them, so this check still looks for the right names.
set -euo pipefail

# wasm-bindgen's names for Url.createObjectURL and an anchor's `download`. (A Blob alone
# isn't a way out: eframe makes them for the clipboard.)
calls=(createObjectURL 'set_\?download')

glue() {
  local js
  js=$(ls "$1"/*.js 2>/dev/null | head -n 1)
  if [ -z "$js" ]; then
    echo "no JavaScript glue in $1" >&2
    exit 1
  fi
  echo "$js"
}

demo=$(glue "$1")
for call in "${calls[@]}"; do
  if grep -q "__wbg_$call" "$demo"; then
    echo "$demo imports $call: the demo can download files" >&2
    exit 1
  fi
done

if [ $# -ge 2 ]; then
  full=$(glue "$2")
  for call in "${calls[@]}"; do
    if ! grep -q "__wbg_$call" "$full"; then
      echo "$full doesn't import $call: update the names this script looks for" >&2
      exit 1
    fi
  done
fi
echo "$demo has no download calls"
