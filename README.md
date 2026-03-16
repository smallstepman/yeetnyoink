# yeet-and-yoink Firefox bridge

This repository contains the Firefox-family WebExtension bridge for `yeet-and-yoink` plus a
per-user native-messaging host manifest template.

## Contents

- `manifest.json` and `background.js`: the extension source tree.
- `build-xpi.sh`: packages the extension files into `dist/yeet-and-yoink-firefox-bridge-<version>.xpi`.
- `native-host/com.yeet_and_yoink.firefox_bridge.json.template`: reference manifest template.

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

Preferred:

```sh
yny setup firefox
```

The installer writes a small `yeet-and-yoink-firefox-host` wrapper next to the manifest and points
the manifest at that wrapper. This is required because Firefox/LibreWolf native-messaging manifests
can only name an executable path, while the main `yny` CLI must be invoked as
`yny browser-host firefox`.

Use `--yny-path /absolute/path/to/yny` if you want `setup firefox` to target a different binary
than the currently running one.

By default the installer writes to:

- Linux Firefox: `~/.mozilla/native-messaging-hosts/`
- Linux LibreWolf: `~/.librewolf/native-messaging-hosts/`
- macOS: `~/Library/Application Support/Mozilla/NativeMessagingHosts/`

Pass an explicit second argument if your browser expects a different manifest directory:

```sh
yny setup firefox --manifest-dir /custom/native-messaging-hosts
```

If LibreWolf shows the extension as installed but `yny` reports that
`/run/user/<uid>/yeet-and-yoink/firefox-bridge.sock` is missing, the usual cause is that the
native host manifest is not present under LibreWolf's manifest directory, so the browser never
launches `yny browser-host firefox`.

## Native host details

- Native host name: `com.yeet_and_yoink.firefox_bridge`
- Extension ID: `browser-bridge@yeet-and-yoink`
