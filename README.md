# HashWatcher for Braiins Deck

Birds Eye fleet, neural view, and miner dashboards as a Deck widget. Miners are sent from the HashWatcher app. This repository is the widget only.

Miner passwords are not in this repository. They are supplied at runtime by the app and are not stored here.

## Install the widget

### From a `.nar` on the Deck webpage

Braiins Deck firmware that supports **Display Widgets → Install Widget** can install HashWatcher from a Nix archive (`.nar`). No SSH required.

1. Download `hashwatcher.nar` from the [latest release](https://github.com/gpena208777/hashwatcher-deck/releases/latest).
2. On a computer, open the Deck’s local webpage (the Deck IP from the settings tray).
3. Go to **Display Widgets → Install Widget**.
4. Accept the third-party notice and choose the `.nar` file.

The widget shows up in the picker right away. To update later, install a newer `.nar` with the same widget `uid` and a higher `version`.

If you previously installed HashWatcher over SSH as a flat `widget-hashwatcher` package, remove that install first. The `.nar` uses the third-party path `lib/bmc-widgets/hashwatcher/hashwatcher/` (package name `widget-hashwatcher_hashwatcher`), and a flat folder named `hashwatcher` would clash.

### Build a `.nar` yourself

Build the WASM inside a [bmc-main](https://github.com/BraiinsForge/bmc-main) checkout at `widgets-wasm/hashwatcher` (this crate links the Deck SDK by relative path):

```bash
cargo build -p hashwatcher --target wasm32-unknown-unknown --release
```

Then from this repo (Nix/`nix-store` required):

```bash
./deck/pack-nar.sh /path/to/bmc-main/target/wasm32-unknown-unknown/release/hashwatcher.wasm
# → dist/hashwatcher.nar
```

Package layout follows [Braiins’ upload guide](https://github.com/BraiinsForge/bmc-main/blob/master/docs/uploading-wasm-widgets.md).

### Install over SSH (fallback)

Paste this to an agent on a computer that can reach the Deck. Swipe down on the Deck and copy the IP address shown there. Put that address where the prompt says `YOUR_DECK_IP`.

```text
Install the HashWatcher widget from https://github.com/gpena208777/hashwatcher-deck on my Braiins Deck at YOUR_DECK_IP.

SSH in as root. The password is blank unless I say otherwise. Build the crate at widgets-wasm/hashwatcher inside a BraiinsForge/bmc-main checkout, because it links the Deck SDK by relative path:

    cargo build -p hashwatcher --target wasm32-unknown-unknown --release

Put the stripped wasm, manifest, and icon in /mnt/data/bmc/widgets/hashwatcher (wasm at lib/wasm/hashwatcher.wasm, icon at assets/icon.svg). Then register that directory as a Deck package with the maintainer layout. Do not symlink it into /nix/var/nix/gcroots/profiles/bmc. That directory is rebuilt from the package list when any other widget updates, and a symlink there is deleted.

curl -fsSL https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck/assemble-package.sh -o /tmp/assemble-package.sh
curl -fsSL https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck/hashwatcher-launcher.sh -o /tmp/hashwatcher-launcher.sh
curl -fsSL https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck/register-package.sh -o /tmp/register-package.sh
chmod +x /tmp/assemble-package.sh /tmp/register-package.sh
# register-package.sh expects assemble-package.sh and the launcher beside it
mkdir -p /tmp/hw-deck && cp /tmp/assemble-package.sh /tmp/hashwatcher-launcher.sh /tmp/register-package.sh /tmp/hw-deck/
sh /tmp/hw-deck/register-package.sh /mnt/data/bmc/widgets/hashwatcher
/etc/init.d/bmc-compositor restart

Add one fullscreen scene for widget uid c4e8a1d2-7b63-4f0e-9a15-6d2b8f0c3e71 if that scene is not already there. Keep every scene and setting already in /etc/bmc/config.json. Leave other widgets installed.

Do not add miners and do not store miner passwords. I will send the fleet from HashWatcher.
```

Or prefer the `.nar` path above when the Deck webpage supports it.

## Add it to HashWatcher

HashWatcher for iOS **1.8.1 or later** is required. Earlier versions cannot send a fleet to the Deck.

Put the iPhone and the Braiins Deck on the same Wi-Fi. A Deck has no username. Leave the password blank unless you set one on the Deck.

After the widget is on a scene, add the Deck in the app using either path below. Then send the inventory. That replaces the roster on the Deck. Logins for Braiins and VNish miners go with the miners that need them.

### Add it from the Braiins device type

1. In HashWatcher, add a device and choose **Braiins**.
2. The form is labeled **Miner/Deck**. Enter a name and the Deck IP address.
3. Leave the password blank unless the Deck has one. The username field is for a Braiins OS miner, whose default username is root. A Deck does not use it.
4. Tap **Connect**.

HashWatcher saves the Deck and turns on the **Braiins Deck Remote** tile on Home.

### Add it from Braiins Deck Remote

1. If that tile is not on Home, open **Settings → Extra Features** and turn on **Braiins Deck Remote**.
2. Open **Braiins Deck Remote**.
3. Tap **Scan for a Deck**, or type the Deck IP address.
4. Leave the password blank unless the Deck has one, then tap **Continue**.

Hold the home tile and choose **Remove from HomeView** to hide it. The saved Deck stays. Turn **Braiins Deck Remote** back on under Extra Features to show the tile again.

A Deck row in Scan All is only a found address. Use one of the two paths above to save it.

## On a Deck

![Fleet view](docs/fleet.jpg)

Fleet view with hashrate, shares, and per-miner cards.

![New best difficulty](docs/best-difficulty.jpg)

Best-difficulty celebration. The popup stays up until Awesome is tapped.

![Best difficulty on the desk](docs/best-difficulty-desk.jpg)

The same celebration with the Deck LEDs running until Awesome is tapped.

## Notes

- Pool commands are read-only. The widget uses the CGMiner `pools` command and HTTP GETs. It does not send `addpool`, `switchpool`, or any other pool change.
- The Outbound traffic setting defaults to on. It is used only for weather data. When it is off, weather lookups stop and the weather screen saver explains why.
- License: GPL-3.0-or-later.
