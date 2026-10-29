# HashWatcher for Braiins Deck

Fleet, neural view, and miner dashboards on your Deck. Miners are sent from the HashWatcher app.

## Install

### 1. If you installed the old SSH build

Remove it before installing the new package. Same Wi‑Fi as the Deck.

1. Open the Deck webpage: [http://braiins-deck.local/](http://braiins-deck.local/)  
   (If that does not open, swipe down on the Deck and use the IP shown there.)
2. Under **Display Widgets**, remove any HashWatcher scenes or widgets.
3. On a computer, SSH in as `root` (password blank unless you set one) and run:

```bash
bmc-nix-cli remove-packages --name widget-hashwatcher
```

Then restart the display:

```bash
/etc/init.d/bmc-compositor restart
```

### 2. Install the widget package

1. Download **[hashwatcher.nar](https://github.com/gpena208777/hashwatcher-deck/releases/latest/download/hashwatcher.nar)**
2. Open [http://braiins-deck.local/](http://braiins-deck.local/)
3. Go to **Display Widgets → Install Widget**
4. Accept the third-party notice and choose the `.nar`
5. Add HashWatcher to a scene from the widget picker

### 3. Re-pair from HashWatcher

After installing (or replacing) the widget, open **HashWatcher → Braiins Remote** and connect to the Deck again, then send your fleet. iOS **1.8.1+** is required.

Phone and Deck must be on the same Wi‑Fi. Leave the Deck password blank unless you set one.

## On a Deck

![Fleet view](docs/fleet.jpg)

Fleet view with hashrate, shares, and per-miner cards.

![New best difficulty](docs/best-difficulty.jpg)

Best-difficulty celebration. The popup stays up until Awesome is tapped.

![Best difficulty on the desk](docs/best-difficulty-desk.jpg)

The same celebration with the Deck LEDs running until Awesome is tapped.
