#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  printf 'usage: %s /absolute/path/to/yny [manifest-dir]\n' "$0" >&2
  exit 64
fi

yny_path=$1
case "$yny_path" in
  /*) ;;
  *)
    printf 'yny path must be absolute\n' >&2
    exit 64
    ;;
esac

if [ "$#" -eq 2 ]; then
  target_dir=$2
else
  case "$(uname -s)" in
    Linux)
      target_dir="$HOME/.mozilla/native-messaging-hosts"
      ;;
    Darwin)
      target_dir="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
      ;;
    *)
      printf 'unsupported platform for default manifest path; pass manifest-dir explicitly\n' >&2
      exit 64
      ;;
  esac
fi

mkdir -p "$target_dir"
manifest_path="$target_dir/com.yeet_and_yoink.firefox_bridge.json"
python3 - "$yny_path" "$manifest_path" <<'PY'
import pathlib
import sys

yny_path = sys.argv[1]
out_path = pathlib.Path(sys.argv[2])
template = pathlib.Path('native-host/com.yeet_and_yoink.firefox_bridge.json.template').read_text(encoding='utf-8')
out_path.write_text(template.replace('__YNY_BINARY__', yny_path), encoding='utf-8')
PY
printf 'Wrote %s\n' "$manifest_path"
