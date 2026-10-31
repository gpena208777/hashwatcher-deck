// Copyright (C) 2026 HashWatcher
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! HashWatcher Birds Eye for the Braiins Deck.
//!
//! Miners are sent from the HashWatcher app. The widget draws the fleet and
//! opens a monitoring dashboard when a miner is tapped.

mod format;

#[cfg(target_arch = "wasm32")]
mod live;

#[cfg(test)]
mod format_tests {
    use super::format::{format_hashrate, format_uptime};

    #[test]
    fn hashrate_uses_terahash_for_a_miner_and_gigahash_for_a_small_one() {
        assert_eq!(format_hashrate(Some(140.2)), "140 TH/s");
        assert_eq!(format_hashrate(Some(0.48)), "480 GH/s");
        assert_eq!(format_hashrate(None), "—");
    }

    #[test]
    fn uptime_rolls_into_days() {
        assert_eq!(format_uptime(Some(90)), "1m");
        assert_eq!(format_uptime(Some(3_700)), "1h 1m");
        assert_eq!(format_uptime(Some(90_000)), "1d 1h");
        assert_eq!(format_uptime(None), "—");
    }
}
