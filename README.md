# yeet-and-yoink Firefox bridge

This repository contains the Firefox-family WebExtension bridge for `yeet-and-yoink` plus a
per-user native-messaging host manifest template.

## Contents

- `manifest.json` and `background.js`: the extension source tree.
- `build-xpi.sh`: packages the extension files into `dist/yeet-and-yoink-firefox-bridge-<version>.xpi`.
- `install-native-host.sh`: installs a user-scoped native host manifest for the `yny` binary.
- `native-host/com.yeet_and_yoink.firefox_bridge.json.template`: template used by the installer.

## Install the extension

For local testing, open `about:debugging#/runtime/this-firefox` and load `manifest.json` as a
temporary add-on.

To build an XPI for self-distribution, run:

```sh
./build-xpi.sh
```

Firefox requires a Mozilla-signed XPI for persistent general-user installs. The generated XPI is
best suited for development, testing, or managed environments.

## Install the native host manifest

```sh
./install-native-host.sh /absolute/path/to/yny
```

By default the script writes to:

- Linux: `~/.mozilla/native-messaging-hosts/`
- macOS: `~/Library/Application Support/Mozilla/NativeMessagingHosts/`

## Native host details

- Native host name: `com.yeet_and_yoink.firefox_bridge`
- Extension ID: `browser-bridge@yeet-and-yoink.dev`
