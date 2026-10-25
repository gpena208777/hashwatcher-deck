# HashWatcher for Braiins Deck

Birds Eye fleet, neural view, and miner dashboards as a Deck widget. Miners are sent from the HashWatcher app. This repository is the widget only.

Miner passwords are not in this repository. They are supplied at runtime by the app and are not stored here.

## Install the widget

Braiins is adding a way to install HashWatcher from the Deck’s local webpage. That update is coming soon. Until it ships, a new Deck does not include this widget.

### Build it

This crate expects to live at `widgets-wasm/hashwatcher` inside [bmc-main](https://github.com/BraiinsForge/bmc-main), because it links the Braiins WASM SDK by relative path.

```bash
cargo build -p hashwatcher --target wasm32-unknown-unknown --release
```

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
