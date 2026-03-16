# yeet-and-yoink Chrome bridge

This repository contains the Chromium-family extension bridge for `yeet-and-yoink` plus a
per-user native-messaging host manifest template.

## Contents

- `manifest.json` and `service_worker.js`: the unpacked extension source tree.
- `native-host/com.yeet_and_yoink.chromium_bridge.json.template`: reference manifest template.

## Install the extension

Open `chrome://extensions`, enable developer mode, and load this repository root as an unpacked
extension.

The embedded extension key keeps the extension ID stable across Chromium-family browsers that honor
it, which is required for native-messaging permissions.

## Install the native host manifest

Preferred:

```sh
yny setup chromium
```

Other supported browser targets are `chrome`, `brave`, and `edge`.

The installer writes a small `yeet-and-yoink-chromium-host` wrapper next to the manifest and points
the manifest at that wrapper. This is required because Chromium-family native-messaging manifests
can only name an executable path, while the main `yny` CLI must be invoked as
`yny browser-host chromium`.

Use `--yny-path /absolute/path/to/yny` if you want `setup chromium` to target a different binary
than the currently running one.

Defaults:

- Linux Chromium: `~/.config/chromium/NativeMessagingHosts/`
- Linux Chrome: `~/.config/google-chrome/NativeMessagingHosts/`
- Linux Brave: `~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/`
- Linux Edge: `~/.config/microsoft-edge/NativeMessagingHosts/`
- macOS browser-specific `~/Library/Application Support/.../NativeMessagingHosts/`

Use `--manifest-dir /custom/NativeMessagingHosts` to override the target directory explicitly.

## Native host details

- Native host name: `com.yeet_and_yoink.chromium_bridge`
- Extension ID: `oigofebnnajpegmncnciacecfhlokkbp`
