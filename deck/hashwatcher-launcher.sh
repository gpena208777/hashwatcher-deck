#!/bin/sh
exec /run/current-profile/bin/bmc-wasm-thin-v0 \
    --wasm "${0%/*}/../lib/wasm/hashwatcher.wasm" \
    --asset-root "${0%/*}/../lib/assets" \
    "$@"
