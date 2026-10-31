# Tailscale button

Installing HashWatcher from the website should install only the widget. Tailscale should not download or start as part of that install.

Tailscale is an opt-in. The user turns it on with the Tailscale button in the widget. That button used to download and start Tailscale. On a Deck that has only installed the widget, the button cannot do it.

## What the button does

`begin_tailscale_install` in `src/live.rs` asks the Deck to start the installer:

1. `http://127.0.0.1:8000/cgi-bin/hashwatcher-install-tailscale`
2. If that is missing, `http://127.0.0.1:9418/cgi-bin/action?install`

A fresh Install Widget install has neither endpoint. The widget then shows “Could not start the Tailscale install.”

The widget cannot start the download itself. Install Widget only accepts the wasm package: the official launcher, `manifest.json`, the wasm module, and non-executable assets. It does not run `deck/register-package.sh`, and it does not allow the package to install a boot service.

## Scripts that do the install

These already live in this repo. They are the system component. They are not part of the widget screen.

| File | Role |
| --- | --- |
| `deck/cgi-bin/hashwatcher-install-tailscale` | Button target. Starts the installer. |
| `deck/install-tailscale.sh` | Downloads the official Tailscale ARM build and starts it. The binary is not stored in the widget. |
| `deck/supervise.sh` | Keeps Tailscale running and writes status for the widget, including subnet approval. |
| `deck/rc.local` | Boot line that starts `supervise.sh`. |
| `deck/register-package.sh` | Plants the scripts and starts the download when Tailscale is missing. Install Widget does not run this. |

## What we need

When the user presses the Tailscale button, allow the widget to install this system component.

That install should be allowed to:

- download the Tailscale binaries
- install the boot service (`supervise.sh` from `rc.local`)
- run the local control endpoint on `127.0.0.1:9418`

Do not do this when the user installs the widget from the website. Only after they tap the button.
