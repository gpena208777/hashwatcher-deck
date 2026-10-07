//! Scan, poll, and draw. The host calls `init`, `render`, and `on_touch`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use bmc_wasm_sdk::led::{self, LedEffect};
use bmc_wasm_sdk::socket::{self, Socket, SocketEvent};
use bmc_wasm_sdk::*;

use crate::format::{
    format_efficiency, format_hashrate, format_power, format_temp, format_uptime,
};

/// iOS miner tile gray (0.25). Kept nearly solid so the nebula does not wash the card out.
const CARD: Color = Color::from_rgba(64, 64, 64, 242);
const MUTED: Color = Color::from_hex(0x9A_B0_A4);
const EMERALD: Color = Color::from_hex(0x00_CC_66);
const GREEN: Color = Color::from_hex(0x00_CC_66);
const AMBER: Color = Color::from_hex(0xFF_FF_00);
const RED: Color = Color::from_hex(0xFF_00_00);
const BLUE: Color = Color::from_hex(0x4D_A3_FF);

const PROBE_TIMEOUT: Duration = Duration::from_millis(1_200);
const MAX_FETCHES: usize = 12;
const MAX_SOCKETS: usize = 3;
/// Render ticks at 400ms. A port-4028 connect that never answers is dropped.
const SOCKET_TICKS: u32 = 12;
const FRAME_MS: u32 = 400;
const POLL_MS: u32 = 8_000;
/// Hashrate values refresh with the poll. Card and summary order does not.
const HASH_ORDER_MS: u32 = 3_600_000;
/// Same window as the orange share LED breathe.
const ALARM_SOUNDS: &[(&str, &str)] = &[
    ("GreenCandleMorning", "Green candle morning"),
    ("KeepCalmAndDca", "Keep calm and DCA"),
    ("KeepCalmAndDcaV2", "Keep calm and DCA V2"),
    ("HashrateMelody", "Hashrate melody"),
    ("TickTockNextBlock", "Tick tock, next block"),
    ("OgStyleWakingUp", "OG style waking up"),
    ("SonarPriceAlert", "Sonar price alert"),
    ("SubtlePriceAlert", "Subtle price alert"),
    ("PriceUp", "Price up"),
    ("PriceDown", "Price down"),
    ("Confirmation", "Confirmation"),
    ("ErrorSound", "Error Sound"),
];
const ALARM_DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const SHARE_FLASH_MS: u32 = 420;
const LED_RUN_MS: u32 = 24_000;
const LED_REARM_MS: u32 = 20_000;
const FLEET_COLUMNS: usize = 4;
const FLEET_CARD_H: f32 = 122.0;
const FLEET_GROUP_H: f32 = 150.0;
const FLEET_GAP: f32 = 6.0;
const FLEET_PAD: f32 = 6.0;
const FLEET_HEADER_H: f32 = 64.0;
const FLEET_METRICS_H: f32 = 86.0;
/// iOS chrome gray (0.16) behind the Hash Watcher / Fleet / Neural / settings row.
const BAR_FILL: Color = Color::from_rgb(41, 41, 41);
const METRIC_FILL: Color = Color::from_rgb(54, 54, 54);
/// Fleet totals sit on a near-black band so they do not read as another miner row.
const TOTALS_FILL: Color = Color::from_rgb(10, 12, 11);
const LABEL: Color = Color::from_hex(0x9E_9E_9E);
const SHARE_ORANGE: Color = Color::from_hex(0xFF_9F_0A);

const EMERALD_BG: Bitmap = include_bitmap!("assets/emeraldtheme.jpg");
const GEAR_ICON: Bitmap = include_bitmap!("assets/gear.png");
const DOWNLOAD_QR: Bitmap = include_bitmap!("assets/hashwatcher-qr.png");
const SUPPORT_QR: Bitmap = include_bitmap!("assets/support-qr.png");
/// Deck settings release. Bump this on each update. The support QR emails the same value.
const DECK_VERSION: &str = "1.0.1";
/// First look is a minute after start, then once an hour. Outbound Data must be on.
const UPDATE_FIRST_MS: u32 = 60_000;
const UPDATE_EVERY_MS: u32 = 3_600_000;
const RELEASE_URL: &str = "https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck/release.json";

fn version_is_newer(remote: &str, local: &str) -> bool {
    let parse = |text: &str| -> [u32; 3] {
        let mut parts = [0u32; 3];
        for (index, part) in text.trim().trim_start_matches('v').split('.').take(3).enumerate() {
            parts[index] = part.parse().unwrap_or(0);
        }
        parts
    };
    parse(remote) > parse(local)
}
/// Shared settings-button chrome. Idle and selected are the only two faces.
/// Each button still has its own label and click id.
const SETTINGS_BUTTON: NinePatchAsset = include_nine_patch!("assets/settings-button.9.png");
const SETTINGS_BUTTON_ON: NinePatchAsset = include_nine_patch!("assets/settings-button-on.9.png");
const TAILSCALE_LOGO: Bitmap = include_bitmap!("assets/tailscale.png");
const TAILSCALE_CAT: Bitmap = include_bitmap!("assets/tailscale-cat.png");
const NUCLEUS_IMG: Bitmap = include_bitmap!("assets/nucleus.png");
const NODE_IMG: Bitmap = include_bitmap!("assets/node-rough.png");
const NODE_IDLE: Bitmap = include_bitmap!("assets/node-rough-idle.png");
const CONTROL_PAUSE: Bitmap = include_bitmap!("assets/control-pause.png");
const CONTROL_RESUME: Bitmap = include_bitmap!("assets/control-resume.png");
const CONTROL_RESTART: Bitmap = include_bitmap!("assets/control-restart.png");
const CLOCK_H: f32 = 184.0;
const CLOCK_0: Bitmap = include_bitmap!("assets/clock/0.png");
const CLOCK_1: Bitmap = include_bitmap!("assets/clock/1.png");
const CLOCK_2: Bitmap = include_bitmap!("assets/clock/2.png");
const CLOCK_3: Bitmap = include_bitmap!("assets/clock/3.png");
const CLOCK_4: Bitmap = include_bitmap!("assets/clock/4.png");
const CLOCK_5: Bitmap = include_bitmap!("assets/clock/5.png");
const CLOCK_6: Bitmap = include_bitmap!("assets/clock/6.png");
const CLOCK_7: Bitmap = include_bitmap!("assets/clock/7.png");
const CLOCK_8: Bitmap = include_bitmap!("assets/clock/8.png");
const CLOCK_9: Bitmap = include_bitmap!("assets/clock/9.png");
const CLOCK_COLON: Bitmap = include_bitmap!("assets/clock/colon.png");
const CLOCK_A: Bitmap = include_bitmap!("assets/clock/A.png");
const CLOCK_M: Bitmap = include_bitmap!("assets/clock/M.png");
const CLOCK_P: Bitmap = include_bitmap!("assets/clock/P.png");
const BEST_DIFF_IMG: Bitmap = include_bitmap!("assets/best-diff.png");
const BEST_DIFF_ALT: Bitmap = include_bitmap!("assets/best-diff-alt.png");
const BLOCK_FOUND_IMG: Bitmap = include_bitmap!("assets/block-found.png");
/// Deck speaker listener for the share coin.
const COIN_PORT: u16 = 19721;
/// iOS neural card gray (white 0.13 at 95%).
const NEURAL_FILL: Color = Color::from_rgba(33, 33, 33, 242);
const POSTER_BG: Color = Color::from_rgb(22, 4, 6);
const POSTER_GOLD: Color = Color::from_rgb(255, 214, 74);
const POSTER_IVORY: Color = Color::from_rgb(255, 245, 199);
const POSTER_MUTED: Color = Color::from_rgba(255, 255, 255, 158);

// Miner queries are read-only. `pools` and the HTTP pool endpoints only return
// the pool list. This widget never sends addpool, switchpool, or any other
// command that changes pool data.
const SUMMARY_CMD: &[u8] = b"{\"command\":\"summary\"}\n";
const VERSION_CMD: &[u8] = b"{\"command\":\"version\"}\n";
const ESTATS_CMD: &[u8] = b"{\"command\":\"estats\"}\n";
const STATS_CMD: &[u8] = b"{\"command\":\"stats\"}\n";
const POOLS_CMD: &[u8] = b"{\"command\":\"pools\"}\n";
const TEMPS_CMD: &[u8] = b"{\"command\":\"temps\"}\n";
const FANS_CMD: &[u8] = b"{\"command\":\"fans\"}\n";
const OUTBOUND_WEATHER_ERROR: &str =
    "Outbound traffic is turned off. It is used only for weather data.";

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
    static TICK: Cell<u32> = const { Cell::new(0) };
    static BEST_QUEUE: RefCell<VecDeque<BestPopup>> = RefCell::new(VecDeque::new());
    static BEST_GRAPHIC: Cell<u8> = const { Cell::new(0) };
}

#[derive(Clone)]
struct Weather {
    label: String,
    lat: f64,
    lon: f64,
    temp_c: f64,
    feels_c: f64,
    wind_kmh: f64,
    wind_deg: Option<f64>,
    humidity: i64,
    code: i64,
    is_day: bool,
    ready: bool,
    fetched_at: i64,
    error: String,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            label: String::new(),
            lat: 0.0,
            lon: 0.0,
            temp_c: 0.0,
            feels_c: 0.0,
            wind_kmh: 0.0,
            wind_deg: None,
            humidity: 0,
            code: 0,
            is_day: true,
            ready: false,
            fetched_at: 0,
            error: String::new(),
        }
    }
}

struct App {
    prefix: String,
    own: Option<u8>,
    queue: VecDeque<Probe>,
    inflight: HashMap<u32, Probe>,
    tcp_queue: VecDeque<u8>,
    sockets: HashMap<u32, SocketJob>,
    miners: Vec<Miner>,
    finished: u16,
    total: u16,
    phase: Phase,
    selected: Option<String>,
    chart_open: bool,
    chart_span: u8,
    chart_saved: i64,
    page: usize,
    message: String,
    notice: String,
    led_label: String,
    /// Last phone `led=` value already applied. A repeat must not turn the strip off again.
    led_remote: String,
    led_rgb: (u8, u8, u8),
    led_effect: LedEffect,
    led_period: u32,
    led_hold: bool,
    led_rainbow: bool,
    led_refresh_ms: u32,
    celebration_led: u8,
    rainbow_ms: u32,
    rainbow_hue: u16,
    fleet_row: usize,
    gesture_y: Option<f32>,
    gesture_moved: bool,
    neural: bool,
    summary: bool,
    summary_cols: [u8; 4],
    summary_edit: u8,
    since_poll: u32,
    neural_spin: f32,
    neural_spin_drawn: f32,
    neural_hint_ms: u32,
    settings: bool,
    poll_ms: u32,
    share_pulse: bool,
    share_sound: bool,
    sound_volume: u8,
    sound_socket: Option<Socket>,
    sound_started: u32,
    celebration: Option<BestPopup>,
    share_flash_ms: u32,
    share_ips: Vec<String>,
    /// 0 names, 1 hashrate only, 2 no labels.
    label_mode: u8,
    fleet_groups: bool,
    /// Miner IPs in hashrate order. Rebuilt at most once an hour.
    hash_order: Vec<String>,
    hash_order_ms: u32,
    hash_order_seeded: bool,
    saver_ms: u32,
    saver_weather: bool,
    /// Internet fetches. Used only for weather data.
    outbound: bool,
    weather_f: bool,
    saver_on: bool,
    idle_ms: u32,
    swallow_touch: bool,
    brightness: u8,
    night_brightness: u8,
    screen_off_secs: Option<u32>,
    screen_off_known: bool,
    deck_token: String,
    deck_fetch: Option<u32>,
    deck_kind: u8,
    deck_wait_ms: u32,
    /// Last `deck` param applied or published, so an echo does not reload the fleet.
    controls_remote: String,
    controls_pending: String,
    controls_dirty: bool,
    applying_remote: bool,
    sync_scene: String,
    sync_widget: String,
    sync_retry_ms: u32,
    inventory_stamp: String,
    inventory_seen: bool,
    /// City was chosen on the Deck. Ignore a phone push that still has the previous city.
    place_local: bool,
    brightness_drag: bool,
    zec_fan: Option<ZecFanJob>,
    zec_fetch: Option<u32>,
    place: String,
    place_edit: bool,
    place_shift: bool,
    places: Vec<PlaceHit>,
    suggest_fetch: Option<u32>,
    weather: Weather,
    weather_fetch: Option<u32>,
    weather_geo: bool,
    alarm_open: bool,
    alarm_naming: bool,
    alarm_hour: u8,
    alarm_minute: u8,
    alarm_pm: bool,
    alarm_name: String,
    /// Monday is bit 0. Sunday is bit 6.
    alarm_days: u8,
    alarm_sound: u8,
    alarm_snooze: bool,
    /// 1 forever, 2 three times, 3 five times.
    alarm_limit: u8,
    alarm_after: u8,
    tailscale_open: bool,
    tailscale_fetch: Option<u32>,
    tailscale_wait_ms: u32,
    tailscale_state: String,
    tailscale_host: String,
    tailscale_ip: String,
    tailscale_url: String,
    tailscale_subnet: String,
    tailscale_expiry: String,
    tailscale_admin: String,
    tailscale_cmd: Option<u32>,
    update_wait_ms: u32,
    update_checked: bool,
    update_fetch: Option<u32>,
    /// Screen state to keep after a tap until status.json catches up.
    tailscale_hold: String,
    tailscale_hold_ms: u32,
    /// 0 menu, 1 display, 2 RGB, 3 layout, 4 alerts, 5 refresh, 6 support.
    settings_page: u8,
}

#[derive(Clone)]
struct PlaceHit {
    name: String,
    label: String,
    lat: f64,
    lon: f64,
}

struct ZecFanJob {
    ip: String,
    user: String,
    pass: String,
    manual: bool,
    percent: i32,
    reboot: bool,
    step: u8,
    realm: String,
    nonce: String,
    qop: bool,
    conf: String,
}

struct Probe {
    host: u8,
    kind: ProbeKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProbeKind {
    Liveness,
    BitaxeRetry,
    Identify(u8),
    Poll,
    /// Extra document for a miner whose main poll does not include the pool.
    Detail,
    /// Exchange a stored username and password for an API token.
    Login,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    NeedNetwork,
    Http,
    Tcp,
    Live,
}

struct SocketJob {
    host: u8,
    started: u32,
    connected: bool,
    poll: bool,
    control: bool,
    payload: Vec<u8>,
    buf: Vec<u8>,
    socket: Socket,
}

#[derive(Clone)]
struct BestPopup {
    name: String,
    pool: String,
    difficulty: f64,
    graphic: u8,
    /// 0 best difficulty, 1 block found.
    kind: u8,
}

#[derive(Clone)]
struct Miner {
    host: u8,
    ip: String,
    family: &'static str,
    name: String,
    model: String,
    mac: String,
    tune: String,
    user: String,
    pass: String,
    token: String,
    /// 0 cooling state, 1 hashboards. Braiins REST fills these after the stats poll.
    detail_step: u8,
    fan_manual: bool,
    asic_pct: Option<f64>,
    hashrate_ths: Option<f64>,
    power_w: Option<f64>,
    temp_c: Option<f64>,
    board_temp: Option<f64>,
    vr_temp: Option<f64>,
    fan: Option<f64>,
    fan_rpm: Option<f64>,
    fan_pcts: [f64; 8],
    fan_rpms: [f64; 8],
    uptime_s: Option<u64>,
    pool: String,
    pool_user: String,
    shares_accepted: Option<u64>,
    /// Accepted shares at the end of the previous poll. A flash requires a higher count.
    shares_seen: Option<u64>,
    /// Braiins pool_stats is the share counter. CGMiner Accepted must not replace it.
    share_locked: bool,
    shares_rejected: Option<u64>,
    best_diff: Option<f64>,
    best_session: Option<f64>,
    frequency: Option<f64>,
    default_frequency: Option<f64>,
    eco_frequency: Option<f64>,
    voltage: Option<f64>,
    reachable: bool,
    misses: u8,
    /// Hashrate samples from background polls, newest last. Kept for 24 hours.
    chart: Vec<ChartSample>,
    chart_dirty: bool,
}

#[derive(Clone, Copy)]
struct ChartSample {
    at: i64,
    hash: f32,
}

const CHART_SPANS: [(i64, &str); 8] = [
    (5 * 60, "5 min"),
    (15 * 60, "15 min"),
    (30 * 60, "30 min"),
    (60 * 60, "1 hour"),
    (3 * 60 * 60, "3 hours"),
    (6 * 60 * 60, "6 hours"),
    (12 * 60 * 60, "12 hours"),
    (24 * 60 * 60, "24 hours"),
];

impl App {
    fn new() -> Self {
        Self {
            prefix: String::new(),
            own: None,
            queue: VecDeque::new(),
            inflight: HashMap::new(),
            tcp_queue: VecDeque::new(),
            sockets: HashMap::new(),
            miners: Vec::new(),
            finished: 0,
            total: 0,
            phase: Phase::NeedNetwork,
            selected: None,
            chart_open: false,
            chart_span: 0,
            chart_saved: 0,
            page: 0,
            message: "Looking for the Deck network".to_owned(),
            notice: String::new(),
            led_label: "Ready".to_owned(),
            led_remote: String::new(),
            led_rgb: (0, 204, 102),
            led_effect: LedEffect::Solid,
            led_period: 1_000,
            led_hold: false,
            led_rainbow: false,
            led_refresh_ms: 0,
            celebration_led: 0,
            rainbow_ms: 0,
            rainbow_hue: 0,
            fleet_row: 0,
            gesture_y: None,
            gesture_moved: false,
            neural: false,
            summary: false,
            summary_cols: [0, 1, 2, 3],
            summary_edit: 0,
            since_poll: POLL_MS,
            neural_spin: 0.0,
            neural_spin_drawn: 0.0,
            neural_hint_ms: 0,
            settings: false,
            poll_ms: POLL_MS,
            share_pulse: true,
            share_sound: true,
            sound_volume: 100,
            sound_socket: None,
            sound_started: 0,
            celebration: None,
            share_flash_ms: 0,
            share_ips: Vec::new(),
            label_mode: 0,
            fleet_groups: false,
            hash_order: Vec::new(),
            hash_order_ms: 0,
            hash_order_seeded: false,
            saver_ms: 0,
            saver_weather: false,
            outbound: true,
            weather_f: false,
            saver_on: false,
            idle_ms: 0,
            swallow_touch: false,
            brightness: 0,
            night_brightness: 0,
            screen_off_secs: None,
            screen_off_known: false,
            deck_token: String::new(),
            deck_fetch: None,
            deck_kind: 0,
            deck_wait_ms: 30_000,
            controls_remote: String::new(),
            controls_pending: String::new(),
            controls_dirty: false,
            applying_remote: false,
            sync_scene: String::new(),
            sync_widget: String::new(),
            sync_retry_ms: 0,
            inventory_stamp: String::new(),
            inventory_seen: false,
            place_local: false,
            brightness_drag: false,
            zec_fan: None,
            zec_fetch: None,
            place: String::new(),
            place_edit: false,
            place_shift: true,
            places: Vec::new(),
            suggest_fetch: None,
            weather: Weather::default(),
            weather_fetch: None,
            weather_geo: true,
            alarm_open: false,
            alarm_naming: false,
            alarm_hour: 7,
            alarm_minute: 0,
            alarm_pm: false,
            alarm_name: String::new(),
            alarm_days: 0b0111_1111,
            alarm_sound: 0,
            alarm_snooze: true,
            alarm_limit: 2,
            alarm_after: 0,
            tailscale_open: false,
            tailscale_fetch: None,
            tailscale_wait_ms: 8_000,
            tailscale_state: String::new(),
            tailscale_host: String::new(),
            tailscale_ip: String::new(),
            tailscale_url: String::new(),
            tailscale_subnet: String::new(),
            tailscale_expiry: String::new(),
            tailscale_admin: String::new(),
            tailscale_cmd: None,
            update_wait_ms: 0,
            update_checked: false,
            update_fetch: None,
            tailscale_hold: String::new(),
            tailscale_hold_ms: 0,
            settings_page: 0,
        }
    }

    fn begin(&mut self) {
        self.load_saver();
        self.apply_inventory();
    }

    fn load_saver(&mut self) {
        if let Some(text) = bmc_wasm_sdk::kv::get_string("saver_ms") {
            if let Ok(ms) = text.parse::<u32>() {
                self.saver_ms = ms;
            }
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("saver_mode") {
            self.saver_weather = text == "weather";
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("outbound") {
            self.outbound = text != "0";
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("place") {
            self.place = text;
            self.place_shift = self.place.is_empty();
        }
        self.weather_f = match bmc_wasm_sdk::kv::get_string("weather_unit").as_deref() {
            Some("f") => true,
            Some("c") => false,
            _ => device_fahrenheit(),
        };
        if let Some(text) = bmc_wasm_sdk::kv::get_string("share_sound") {
            self.share_sound = text != "0";
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("sound_volume") {
            if let Ok(volume) = text.parse::<u8>() {
                self.sound_volume = clamp_volume(volume);
            }
        } else if !self.share_sound {
            self.sound_volume = 0;
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("show_names") {
            self.label_mode = if text == "2" {
                1
            } else if text == "0" {
                2
            } else {
                0
            };
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("fleet_groups") {
            self.fleet_groups = text == "1";
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("poll_s") {
            if let Ok(seconds) = text.parse::<u32>() {
                self.poll_ms = seconds.saturating_mul(1_000).clamp(1_000, 60_000);
            }
        }
        if let Some(text) = bmc_wasm_sdk::kv::get_string("summary_cols") {
            let parsed = parse_summary_cols(&text);
            if parsed.len() == 4 {
                self.summary_cols = [parsed[0], parsed[1], parsed[2], parsed[3]];
            }
        }
    }

    fn flush_charts(&mut self, force: bool) {
        let now = SystemTime::now().unix_secs;
        if !force && now.saturating_sub(self.chart_saved) < 60 {
            return;
        }
        if !self.miners.iter().any(|miner| miner.chart_dirty) {
            return;
        }
        self.chart_saved = now;
        for miner in &mut self.miners {
            if miner.chart_dirty {
                save_chart(&miner.ip, &miner.chart);
                miner.chart_dirty = false;
            }
        }
    }

    fn store_poll(&mut self) {
        bmc_wasm_sdk::kv::set("poll_s", (self.poll_ms / 1_000).to_string().as_bytes());
        self.touch_controls();
    }

    fn store_saver(&mut self) {
        bmc_wasm_sdk::kv::set("saver_ms", self.saver_ms.to_string().as_bytes());
        bmc_wasm_sdk::kv::set(
            "saver_mode",
            if self.saver_weather { b"weather" } else { b"clock" },
        );
        bmc_wasm_sdk::kv::set("outbound", if self.outbound { b"1" } else { b"0" });
        bmc_wasm_sdk::kv::set("place", self.place.as_bytes());
        bmc_wasm_sdk::kv::set("weather_unit", if self.weather_f { b"f" } else { b"c" });
        bmc_wasm_sdk::kv::set("share_sound", if self.share_sound { b"1" } else { b"0" });
        bmc_wasm_sdk::kv::set("sound_volume", self.sound_volume.to_string().as_bytes());
        bmc_wasm_sdk::kv::set(
            "show_names",
            match self.label_mode {
                1 => b"2".as_slice(),
                2 => b"0".as_slice(),
                _ => b"1".as_slice(),
            },
        );
        bmc_wasm_sdk::kv::set("fleet_groups", if self.fleet_groups { b"1" } else { b"0" });
        self.store_poll();
        let cols = self
            .summary_cols
            .iter()
            .map(|col| summary_col_id(*col))
            .collect::<Vec<_>>()
            .join(",");
        bmc_wasm_sdk::kv::set("summary_cols", cols.as_bytes());
        self.touch_controls();
    }

    fn touch_controls(&mut self) {
        if !self.applying_remote {
            self.controls_dirty = true;
        }
    }

    fn apply_inventory(&mut self) {
        self.inventory_stamp = inventory_param_stamp();
        self.inventory_seen = true;
        self.cancel_work();
        self.queue.clear();
        self.tcp_queue.clear();
        let info = bmc_wasm_sdk::network::info();
        if let Some((prefix, own)) = split_v4(&info.ip) {
            self.prefix = prefix;
            self.own = Some(own);
        }
        let imported = imported_miners(self.own);
        self.apply_deck_controls();
        if imported.is_empty() {
            self.miners.clear();
            self.selected = None;
            self.phase = Phase::NeedNetwork;
            self.message = "Send miners from HashWatcher".to_owned();
            return;
        }
        let mut next = Vec::new();
        for mut record in imported {
            if let Some(existing) = self.miners.iter().find(|miner| miner.ip == record.ip) {
                record.hashrate_ths = existing.hashrate_ths;
                record.power_w = existing.power_w;
                record.temp_c = existing.temp_c;
                record.board_temp = existing.board_temp;
                record.vr_temp = existing.vr_temp;
                record.fan = existing.fan;
                record.fan_rpm = existing.fan_rpm;
                record.fan_pcts = existing.fan_pcts;
                record.fan_rpms = existing.fan_rpms;
                record.fan_manual = existing.fan_manual;
                record.asic_pct = existing.asic_pct;
                record.uptime_s = existing.uptime_s;
                record.pool = existing.pool.clone();
                record.pool_user = existing.pool_user.clone();
                record.shares_accepted = existing.shares_accepted;
                record.shares_seen = existing.shares_seen;
                record.share_locked = existing.share_locked;
                record.shares_rejected = existing.shares_rejected;
                record.best_diff = existing.best_diff;
                record.best_session = existing.best_session;
                record.frequency = existing.frequency.or(record.frequency);
                record.voltage = existing.voltage.or(record.voltage);
                record.reachable = existing.reachable;
                record.misses = existing.misses;
                record.chart = existing.chart.clone();
                record.chart_dirty = existing.chart_dirty;
                if record.model.is_empty() {
                    record.model = existing.model.clone();
                }
                if record.user == existing.user && record.pass == existing.pass {
                    record.token = existing.token.clone();
                }
            }
            if record.chart.is_empty() && !record.chart_dirty {
                record.chart = load_chart(&record.ip);
            }
            next.push(record);
        }
        next.sort_by(|left, right| left.ip.cmp(&right.ip));
        self.miners = next;
        self.phase = Phase::Live;
        self.message = format!("{} from HashWatcher", self.miners.len());
        self.since_poll = POLL_MS;
    }

    fn maintain_hash_order(&mut self, delta_ms: u32) {
        if self.miners.is_empty() {
            self.hash_order.clear();
            self.hash_order_ms = 0;
            self.hash_order_seeded = false;
            return;
        }
        let polling = !self.queue.is_empty() || !self.inflight.is_empty() || !self.sockets.is_empty();
        if !self.hash_order_seeded {
            if !polling && self.miners.iter().any(|miner| miner.hashrate_ths.is_some()) {
                self.rebuild_hash_order();
            } else if self.hash_order.is_empty() {
                self.sync_hash_order_membership();
            }
            return;
        }
        self.hash_order_ms = self.hash_order_ms.saturating_add(delta_ms);
        if !polling && self.hash_order_ms >= HASH_ORDER_MS {
            self.rebuild_hash_order();
            return;
        }
        let same = self.hash_order.len() == self.miners.len()
            && self.miners.iter().all(|miner| self.hash_order.iter().any(|ip| ip == &miner.ip));
        if !same {
            self.sync_hash_order_membership();
        }
    }

    fn rebuild_hash_order(&mut self) {
        let mut ips: Vec<String> = if self.hash_order.is_empty() {
            self.miners.iter().map(|miner| miner.ip.clone()).collect()
        } else {
            let mut ordered: Vec<String> = self
                .hash_order
                .iter()
                .filter(|ip| self.miners.iter().any(|miner| miner.ip == **ip))
                .cloned()
                .collect();
            for miner in &self.miners {
                if !ordered.iter().any(|ip| ip == &miner.ip) {
                    ordered.push(miner.ip.clone());
                }
            }
            ordered
        };
        let ranks: Vec<f64> = ips
            .iter()
            .map(|ip| {
                self.miners
                    .iter()
                    .find(|miner| miner.ip == *ip)
                    .map(hash_rank)
                    .unwrap_or(-1.0)
            })
            .collect();
        let mut indexes: Vec<usize> = (0..ips.len()).collect();
        indexes.sort_by(|&left, &right| ranks[right].partial_cmp(&ranks[left]).unwrap_or(std::cmp::Ordering::Equal));
        self.hash_order = indexes.into_iter().map(|index| ips[index].clone()).collect();
        self.hash_order_ms = 0;
        self.hash_order_seeded = true;
    }

    fn sync_hash_order_membership(&mut self) {
        let ips: Vec<String> = self.miners.iter().map(|miner| miner.ip.clone()).collect();
        self.hash_order.retain(|ip| ips.iter().any(|item| item == ip));
        for ip in ips {
            if !self.hash_order.iter().any(|item| item == &ip) {
                self.hash_order.push(ip);
            }
        }
    }

    fn apply_deck_controls(&mut self) {
        let snap = bmc_wasm_sdk::params::current();
        let Some(raw) = snap.get_str("deck") else {
            return;
        };
        let raw = raw.trim();
        if raw.is_empty() || raw == self.controls_remote {
            return;
        }
        self.applying_remote = true;
        let mut brightness = None;
        let mut led = None;
        let mut city = None;
        let mut latitude = None;
        let mut longitude = None;
        let parts: Vec<&str> = if raw.contains(';') {
            raw.split(';').collect()
        } else {
            raw.split(',').collect()
        };
        for part in parts {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key {
                "vol" => {
                    if let Ok(volume) = value.parse::<u8>() {
                        self.sound_volume = clamp_volume(volume);
                    }
                }
                "beep" => self.share_sound = value != "0",
                "poll" => {
                    if let Ok(seconds) = value.parse::<u32>() {
                        self.poll_ms = seconds.saturating_mul(1_000).clamp(1_000, 60_000);
                    }
                }
                "out" => {
                    let on = value != "0";
                    if on != self.outbound {
                        self.outbound = on;
                    }
                }
                "pulse" => self.share_pulse = value != "0",
                "unit" => self.weather_f = !value.eq_ignore_ascii_case("c"),
                "bright" => {
                    if let Ok(percent) = value.parse::<u8>() {
                        brightness = Some(snap_brightness_pct(percent));
                    }
                }
                "saver" => {
                    if let Ok(seconds) = value.parse::<u32>() {
                        self.saver_ms = seconds.saturating_mul(1_000);
                        self.idle_ms = 0;
                        self.saver_on = false;
                    }
                }
                "mode" => self.saver_weather = value.eq_ignore_ascii_case("weather"),
                "led" => led = Some(value.to_owned()),
                "cols" => self.apply_summary_cols(value),
                "city" => city = Some(value.trim().to_owned()),
                "lat" => latitude = value.parse::<f64>().ok(),
                "lon" => longitude = value.parse::<f64>().ok(),
                "view" => self.apply_remote_view(value),
                "names" => {
                    self.label_mode = match value {
                        "0" => 2,
                        "2" => 1,
                        _ => 0,
                    };
                }
                "groups" => {
                    self.fleet_groups = value == "1";
                    self.fleet_row = 0;
                }
                _ => {}
            }
        }
        if let Some(percent) = brightness {
            self.brightness = percent;
            self.night_brightness = percent;
            self.push_brightness();
        }
        let incoming_city = city.clone().unwrap_or_default();
        let previous_city = payload_value(&self.controls_remote, "city");
        let stale_city = self.place_local && incoming_city == previous_city && incoming_city != self.place;
        if stale_city {
            city = None;
            latitude = None;
            longitude = None;
        } else if incoming_city == self.place {
            self.place_local = false;
        }
        if let Some(name) = city {
            if name.is_empty() {
                self.place.clear();
                self.weather = Weather::default();
                self.weather_geo = false;
            } else if name != self.place {
                self.place = name;
                self.weather = Weather::default();
                self.weather_geo = true;
            }
        }
        if self.place.is_empty() {
            latitude = None;
            longitude = None;
        }
        if let (Some(lat), Some(lon)) = (latitude, longitude) {
            if lat.is_finite() && lon.is_finite() && (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
                self.weather.lat = lat;
                self.weather.lon = lon;
                self.weather_geo = false;
                self.weather.ready = false;
                self.weather.fetched_at = 0;
            }
        }
        if let Some(name) = led {
            let fresh = name != self.led_remote;
            if fresh {
                self.led_remote = name.clone();
            }
            // A saved Off was reapplied on every launch, so the strip never
            // left stop. Honor Off only after a color is already showing.
            let showing = self.led_hold || self.led_label != "Ready";
            if fresh && (name != "off" || showing) {
                if let Some((id, rgb)) = Self::parse_custom_led(&name) {
                    self.led_rgb = rgb;
                    let _ = self.apply_led(id);
                } else {
                    let id = match name.as_ref() {
                        "green" => "led-emerald",
                        "white" => "led-white",
                        "amber" => "led-amber",
                        "red" => "led-red",
                        "blue" => "led-blue",
                        "purple" => "led-purple",
                        "off" => "led-off",
                        "breathe" => "led-breathe",
                        "chase" => "led-chase",
                        "scan" => "led-scan",
                        "snake" => "led-snake",
                        "rider" => "led-rider",
                        "rainbow" => "led-rainbow",
                        _ => "",
                    };
                    if !id.is_empty() {
                        let _ = self.apply_led(id);
                    }
                }
            }
        }
        self.store_saver();
        self.applying_remote = false;
        self.controls_remote = raw.to_owned();
        self.controls_dirty = stale_city;
    }

    /// Phone custom mode: `effect:RRGGBB`, for example `breathe:FF5000`.
    fn parse_custom_led(name: &str) -> Option<(&'static str, (u8, u8, u8))> {
        let (effect, hex) = name.trim().split_once(':')?;
        let id = match effect {
            "solid" => "led-solid",
            "breathe" => "led-breathe",
            "chase" => "led-chase",
            "scan" => "led-scan",
            "snake" => "led-snake",
            "rider" => "led-rider",
            _ => return None,
        };
        let hex = hex.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some((id, (red, green, blue)))
    }

    fn apply_summary_cols(&mut self, value: &str) {
        let ids: Vec<&str> = value.split(',').map(str::trim).filter(|id| !id.is_empty()).collect();
        if ids.len() != 4 {
            return;
        }
        let mut cols = [0u8; 4];
        let mut used = [false; 16];
        for (index, id) in ids.iter().enumerate() {
            let Some(col) = summary_col_from_id(id) else {
                return;
            };
            if used[col as usize] {
                return;
            }
            used[col as usize] = true;
            cols[index] = col;
        }
        self.summary_cols = cols;
    }

    fn apply_remote_view(&mut self, value: &str) {
        self.settings = false;
        self.saver_on = false;
        self.selected = None;
        self.idle_ms = 0;
        match value {
            "neural" => {
                let entering = !self.neural || self.summary;
                self.neural = true;
                self.summary = false;
                if entering {
                    self.neural_hint_ms = 2_450;
                }
            }
            "summary" => {
                self.summary = true;
                self.neural = false;
                self.neural_hint_ms = 0;
            }
            _ => {
                self.neural = false;
                self.summary = false;
                self.neural_hint_ms = 0;
            }
        }
    }

    fn cancel_work(&mut self) {
        for raw in self.inflight.keys().copied().collect::<Vec<_>>() {
            if let Some(id) = FetchRequestId::from_wire(raw) {
                let _ = cancel(id);
            }
        }
        self.inflight.clear();
        for job in self.sockets.values() {
            job.socket.close();
        }
        self.sockets.clear();
    }

    fn fill(&mut self) {
        while self.inflight.len() < MAX_FETCHES {
            let Some(probe) = self.queue.pop_front() else {
                break;
            };
            let url = self.url_for(&probe);
            let timeout = match probe.kind {
                ProbeKind::Poll | ProbeKind::Detail | ProbeKind::Login => Duration::from_millis(4_000),
                _ => PROBE_TIMEOUT,
            };
            let creds = self.miners.iter().find(|miner| miner.host == probe.host).map(|miner| {
                (miner.family, miner.user.clone(), miner.pass.clone(), miner.token.clone())
            });
            let (family, mut user, pass, token) = creds.unwrap_or(("", String::new(), String::new(), String::new()));
            if family == "braiins" && user.is_empty() {
                user = "root".to_owned();
            }
            let body = if probe.kind == ProbeKind::Login {
                if family == "vnish" {
                    format!(r#"{{"pw":"{}"}}"#, json_escape(&pass))
                } else {
                    format!(
                        r#"{{"username":"{}","password":"{}"}}"#,
                        json_escape(&user),
                        json_escape(&pass)
                    )
                }
            } else {
                String::new()
            };
            let headers = if probe.kind == ProbeKind::Login {
                "Content-Type: application/json".to_owned()
            } else if matches!(probe.kind, ProbeKind::Poll | ProbeKind::Detail) && !token.is_empty() {
                format!("Authorization: {token}")
            } else {
                String::new()
            };
            let id = if probe.kind == ProbeKind::Login {
                FetchRequest::post(&url)
                    .headers(&headers)
                    .body(body.as_bytes())
                    .timeout(timeout)
                    .send(on_fetch)
            } else if headers.is_empty() {
                FetchRequest::get(&url).timeout(timeout).send(on_fetch)
            } else {
                FetchRequest::get(&url)
                    .headers(&headers)
                    .timeout(timeout)
                    .send(on_fetch)
            };
            let Some(id) = id else {
                self.queue.push_front(probe);
                break;
            };
            self.inflight.insert(id.to_wire(), probe);
        }
        self.fill_sockets();
        if self.phase == Phase::Http && self.queue.is_empty() && self.inflight.is_empty() {
            self.phase = Phase::Tcp;
            self.message = "Checking CGMiner port 4028".to_owned();
        }
        if self.phase == Phase::Tcp && self.tcp_queue.is_empty() && self.sockets.is_empty() {
            self.phase = Phase::Live;
            self.message = format!("{} miners", self.miners.len());
            self.enqueue_polls();
        }
        self.release_shares();
    }

    fn fill_sockets(&mut self) {
        if self.phase != Phase::Tcp && self.phase != Phase::Live {
            return;
        }
        while self.sockets.len() < MAX_SOCKETS {
            let Some(host) = self.tcp_queue.pop_front() else {
                break;
            };
            let ip = self.ip(host);
            let Some(socket) = socket::tcp_connect(&ip, 4028, on_socket) else {
                self.tcp_queue.push_front(host);
                break;
            };
            self.sockets.insert(
                socket.0.to_wire(),
                SocketJob {
                    host,
                    started: TICK.get(),
                connected: false,
                poll: true,
                control: false,
                payload: Vec::new(),
                buf: Vec::new(),
                socket,
            },
        );
    }
    }

    fn url_for(&self, probe: &Probe) -> String {
        let ip = self.ip(probe.host);
        match probe.kind {
            ProbeKind::Liveness | ProbeKind::BitaxeRetry | ProbeKind::Identify(0) => {
                format!("http://{ip}/api/system/info")
            }
            ProbeKind::Identify(1) => format!("http://{ip}/api/v1/status"),
            ProbeKind::Identify(2) => format!("http://{ip}/api/v1/version"),
            ProbeKind::Identify(3) => format!("http://{ip}/api/v1/info"),
            ProbeKind::Identify(4) => format!("http://{ip}/api/overview"),
            ProbeKind::Identify(5) => format!("http://{ip}/api/device/info"),
            ProbeKind::Identify(_) => format!("http://{ip}:5000/health"),
            ProbeKind::Poll => self.poll_url(probe.host),
            ProbeKind::Detail => self.detail_url(probe.host),
            ProbeKind::Login => {
                let family = self
                    .miners
                    .iter()
                    .find(|miner| miner.host == probe.host)
                    .map(|miner| miner.family)
                    .unwrap_or("");
                if family == "vnish" {
                    format!("http://{ip}/api/v1/unlock")
                } else {
                    format!("http://{ip}/api/v1/auth/login")
                }
            }
        }
    }

    fn detail_url(&self, host: u8) -> String {
        let ip = self.ip(host);
        let family = self
            .miners
            .iter()
            .find(|miner| miner.host == host)
            .map(|miner| miner.family)
            .unwrap_or("");
        match family {
            "harlo" => format!("http://{ip}/api/v1/pool"),
            "bitaxe" | "luckyMiner" => format!("http://{ip}/api/overview"),
            "braiins" => match self
                .miners
                .iter()
                .find(|miner| miner.host == host)
                .map(|miner| miner.detail_step)
                .unwrap_or(0)
            {
                0 => format!("http://{ip}/api/v1/cooling/state"),
                _ => format!("http://{ip}/api/v1/miner/hw/hashboards"),
            },
            _ => format!("http://{ip}/"),
        }
    }

    fn queue_detail(&mut self, host: u8) {
        let pending = self
            .queue
            .iter()
            .any(|probe| probe.host == host && probe.kind == ProbeKind::Detail)
            || self
                .inflight
                .values()
                .any(|probe| probe.host == host && probe.kind == ProbeKind::Detail);
        if !pending {
            self.queue.push_back(Probe {
                host,
                kind: ProbeKind::Detail,
            });
        }
    }

    fn poll_url(&self, host: u8) -> String {
        let ip = self.ip(host);
        let family = self
            .miners
            .iter()
            .find(|miner| miner.host == host)
            .map(|miner| miner.family)
            .unwrap_or("");
        let path = match family {
            "bitaxe" => "/api/system/info",
            "harlo" => "/api/v1/status",
            "braiins" => "/api/v1/miner/stats",
            "vnish" => "/api/v1/summary",
            "luckyMiner" => "/api/system/info",
            "zyberos" => "/api/device/info",
            _ => "/",
        };
        format!("http://{ip}{path}")
    }

    fn ip(&self, host: u8) -> String {
        if let Some(miner) = self
            .miners
            .iter()
            .find(|miner| miner.host == host && !miner.ip.is_empty())
        {
            return miner.ip.clone();
        }
        format!("{}{host}", self.prefix)
    }

    fn poll_deck(&mut self, delta_ms: u32) {
        if self.brightness_drag {
            return;
        }
        if self.deck_fetch.is_some() {
            return;
        }
        if self.controls_dirty && !self.deck_token.is_empty() {
            if self.sync_retry_ms > 0 {
                self.sync_retry_ms = self.sync_retry_ms.saturating_sub(delta_ms.min(2_000));
            } else {
                self.publish_controls();
                return;
            }
        }
        self.deck_wait_ms = self.deck_wait_ms.saturating_add(delta_ms.min(2_000));
        let due = if self.deck_token.is_empty() { 8_000 } else { 30_000 };
        if self.deck_wait_ms < due {
            return;
        }
        self.deck_wait_ms = 0;
        if self.deck_token.is_empty() {
            self.deck_login();
        } else {
            self.deck_read();
        }
    }

    fn deck_login(&mut self) {
        self.deck_send(1, "/braiins.bmc.web.AuthenticationService/Login", grpc_frame(&[]), false);
    }

    fn deck_read(&mut self) {
        self.deck_send(2, "/braiins.bmc.web.ConfigurationService/GetDisplaySettings", grpc_frame(&[]), true);
    }

    fn push_brightness(&mut self) {
        if self.deck_token.is_empty() || self.deck_fetch.is_some() {
            return;
        }
        let night = deck_night();
        let pct = if night { self.night_brightness } else { self.brightness };
        if pct < 10 {
            return;
        }
        let path = if night {
            "/braiins.bmc.web.ConfigurationService/SetBrightnessNightmode"
        } else {
            "/braiins.bmc.web.ConfigurationService/SetBrightness"
        };
        self.deck_send(3, path, grpc_frame(&uint32_value(u32::from(pct))), true);
    }

    fn deck_send(&mut self, kind: u8, path: &str, body: Vec<u8>, authed: bool) {
        let headers = if authed {
            format!(
                "Content-Type: application/grpc-web+proto\nX-Grpc-Web: 1\nCookie: session_id={}",
                self.deck_token
            )
        } else {
            "Content-Type: application/grpc-web+proto\nX-Grpc-Web: 1".to_owned()
        };
        let Some(id) = FetchRequest::post(&format!("http://127.0.0.1{path}"))
            .headers(&headers)
            .body(&body)
            .timeout(Duration::from_millis(2_500))
            .send(on_fetch)
        else {
            return;
        };
        self.deck_fetch = Some(id.to_wire());
        self.deck_kind = kind;
    }

    fn publish_controls(&mut self) {
        if self.sync_widget.is_empty() || self.sync_scene.is_empty() {
            self.deck_send(
                6,
                "/braiins.bmc.web.SceneManagementService/GetScenes",
                grpc_frame(&[]),
                true,
            );
            return;
        }
        let wire = self.controls_wire();
        if wire == self.controls_remote {
            self.controls_dirty = false;
            return;
        }
        self.controls_pending = wire.clone();
        let body = widget_update(&self.sync_widget, &self.sync_scene, &wire);
        self.deck_send(
            7,
            "/braiins.bmc.web.SceneManagementService/UpdateWidget",
            grpc_frame(&body),
            true,
        );
    }

    fn controls_wire(&self) -> String {
        let led = if !self.led_hold {
            "off".to_owned()
        } else if self.led_rainbow {
            "rainbow".to_owned()
        } else {
            let effect = match self.led_effect {
                LedEffect::Solid => "solid",
                LedEffect::Breathe => "breathe",
                LedEffect::Chase => "chase",
                LedEffect::Scan => "scan",
                LedEffect::Snake => "snake",
                LedEffect::KnightRider => "rider",
            };
            let (red, green, blue) = self.led_rgb;
            format!("{effect}:{red:02X}{green:02X}{blue:02X}")
        };
        let names = match self.label_mode {
            1 => "2",
            2 => "0",
            _ => "1",
        };
        let view = if self.neural {
            "neural"
        } else if self.summary {
            "summary"
        } else {
            "fleet"
        };
        let city = self
            .place
            .replace([';', '='], " ")
            .trim()
            .to_owned();
        let cols = self
            .summary_cols
            .iter()
            .map(|col| summary_col_id(*col))
            .collect::<Vec<_>>()
            .join(",");
        let mut fields = vec![
            format!("vol={}", self.sound_volume),
            format!("poll={}", self.poll_ms / 1_000),
            format!("pulse={}", if self.share_pulse { "1" } else { "0" }),
            format!("beep={}", if self.share_sound { "1" } else { "0" }),
            format!("unit={}", if self.weather_f { "f" } else { "c" }),
            format!("saver={}", self.saver_ms / 1_000),
            format!("mode={}", if self.saver_weather { "weather" } else { "clock" }),
            format!("led={led}"),
            format!("cols={cols}"),
            format!("city={city}"),
            format!("view={view}"),
            format!("names={names}"),
            format!("groups={}", if self.fleet_groups { "1" } else { "0" }),
            format!("out={}", if self.outbound { "1" } else { "0" }),
        ];
        let bright = shown_brightness(self);
        if bright >= 10 {
            fields.insert(5, format!("bright={bright}"));
        }
        if !city.is_empty() && self.weather.lat.is_finite() && self.weather.lon.is_finite() {
            fields.push(format!("lat={:.4}", self.weather.lat));
            fields.push(format!("lon={:.4}", self.weather.lon));
        }
        fields.join(";")
    }

    fn poll_tailscale(&mut self, delta_ms: u32) {
        if !self.settings || self.tailscale_fetch.is_some() {
            return;
        }
        if !self.tailscale_hold.is_empty() {
            self.tailscale_hold_ms = self.tailscale_hold_ms.saturating_add(delta_ms.min(2_000));
        }
        self.tailscale_wait_ms = self.tailscale_wait_ms.saturating_add(delta_ms.min(2_000));
        let due = if !self.tailscale_hold.is_empty() {
            700
        } else if self.tailscale_open {
            2_000
        } else {
            6_000
        };
        if self.tailscale_wait_ms < due {
            return;
        }
        self.tailscale_wait_ms = 0;
        let Some(id) = FetchRequest::get("http://127.0.0.1:9418/status.json")
            .timeout(Duration::from_millis(2_000))
            .send(on_fetch)
        else {
            return;
        };
        self.tailscale_fetch = Some(id.to_wire());
    }

    fn finish_tailscale(&mut self, response: &FetchResponse) {
        if !response.ok() {
            if self.tailscale_hold.is_empty() {
                self.tailscale_state = "offline".to_owned();
                self.tailscale_url.clear();
                self.tailscale_subnet.clear();
                self.tailscale_expiry.clear();
                self.tailscale_admin.clear();
            }
            return;
        }
        let doc = response.json();
        let incoming = doc.str("/state").unwrap_or_else(|| "offline".to_owned());
        if !self.tailscale_hold.is_empty() {
            let settled = match self.tailscale_hold.as_str() {
                "starting" => matches!(incoming.as_str(), "starting" | "connected" | "needs_login"),
                "stopped" => incoming == "stopped" || incoming == "offline",
                _ => incoming == self.tailscale_hold,
            } || self.tailscale_hold_ms > 8_000;
            if !settled {
                return;
            }
            self.tailscale_hold.clear();
        }
        self.tailscale_state = incoming;
        self.tailscale_host = doc.str("/hostname").unwrap_or_default();
        self.tailscale_ip = doc.str("/ip").unwrap_or_default();
        let login_url = doc.str("/login_url").unwrap_or_default();
        self.tailscale_subnet = doc.str("/subnet").unwrap_or_default();
        self.tailscale_expiry = doc.str("/key_expiry").unwrap_or_default();
        self.tailscale_admin = doc.str("/admin_url").unwrap_or_default();
        if self.tailscale_state == "needs_login" {
            if !login_url.is_empty() {
                self.tailscale_url = login_url;
            }
        } else {
            self.tailscale_url.clear();
        }
    }

    fn tailscale_command(&mut self, action: &str) {
        if self.tailscale_cmd.is_some() {
            return;
        }
        let url = format!("http://127.0.0.1:9418/cgi-bin/action?{action}");
        let Some(id) = FetchRequest::get(&url)
            .timeout(Duration::from_millis(2_000))
            .send(on_fetch)
        else {
            return;
        };
        self.tailscale_cmd = Some(id.to_wire());
        self.tailscale_state = if action == "stop" {
            "stopped".to_owned()
        } else if action == "reset" {
            "needs_login".to_owned()
        } else {
            "starting".to_owned()
        };
        self.tailscale_hold = if action == "stop" {
            "stopped".to_owned()
        } else if action == "start" {
            "starting".to_owned()
        } else if action == "reset" {
            "needs_login".to_owned()
        } else {
            String::new()
        };
        self.tailscale_hold_ms = 0;
        if action == "reset" {
            self.tailscale_ip.clear();
            self.tailscale_subnet.clear();
            self.tailscale_expiry.clear();
            self.tailscale_admin.clear();
            self.tailscale_url.clear();
        } else if action != "relogin" {
            self.tailscale_url.clear();
        }
        self.tailscale_wait_ms = 0;
    }

    fn finish_deck(&mut self, response: &FetchResponse) {
        let kind = self.deck_kind;
        self.deck_kind = 0;
        if !response.ok() {
            if kind == 4 {
                self.notice = "Alarm was not saved".to_owned();
            }
            if kind != 1 && kind != 4 && kind != 5 {
                self.deck_token.clear();
                self.deck_wait_ms = 8_000;
            }
            return;
        }
        let msg = grpc_message(response.body());
        match kind {
            1 => {
                if let Some(token) = proto_string(msg, 1) {
                    if !token.is_empty() {
                        self.deck_token = token;
                        self.deck_wait_ms = 30_000;
                        self.deck_flush_alarm();
                    }
                }
            }
            2 => self.apply_display(msg),
            6 => {
                if let Some((scene, widget)) = find_hashwatcher_widget(msg) {
                    self.sync_scene = scene;
                    self.sync_widget = widget;
                    self.sync_retry_ms = 0;
                } else {
                    self.sync_retry_ms = 8_000;
                }
            }
            7 => {
                self.controls_remote = self.controls_pending.clone();
                self.controls_dirty = self.controls_wire() != self.controls_remote;
            }
            4 => {
                self.notice = "Alarm added".to_owned();
                self.alarm_open = false;
            }
            _ => {}
        }
    }

    fn apply_display(&mut self, msg: &[u8]) {
        if !self.brightness_drag {
            if let Some(day) = proto_bytes(msg, 1).and_then(|nested| proto_varint(nested, 1)) {
                self.brightness = day.clamp(10, 100) as u8;
            }
            if let Some(night) = proto_bytes(msg, 2).and_then(|nested| proto_varint(nested, 1)) {
                self.night_brightness = night.clamp(10, 100) as u8;
            }
        }
        self.screen_off_known = true;
        self.screen_off_secs = proto_bytes(msg, 5)
            .and_then(|nested| proto_varint(nested, 1))
            .map(|value| value as u32);
    }

    fn handle_http(&mut self, response: &FetchResponse) {
        if self.zec_fetch == Some(response.request_id.to_wire()) {
            self.zec_fetch = None;
            self.finish_zec(response);
            return;
        }
        if self.deck_fetch == Some(response.request_id.to_wire()) {
            self.deck_fetch = None;
            self.finish_deck(response);
            return;
        }
        if self.tailscale_fetch == Some(response.request_id.to_wire()) {
            self.tailscale_fetch = None;
            self.finish_tailscale(response);
            return;
        }
        if self.tailscale_cmd == Some(response.request_id.to_wire()) {
            self.tailscale_cmd = None;
            return;
        }
        if self.suggest_fetch == Some(response.request_id.to_wire()) {
            self.suggest_fetch = None;
            self.finish_suggestions(response);
            return;
        }
        if self.weather_fetch == Some(response.request_id.to_wire()) {
            self.weather_fetch = None;
            self.finish_weather(response);
            return;
        }
        if self.update_fetch == Some(response.request_id.to_wire()) {
            self.update_fetch = None;
            self.finish_update_check(response);
            return;
        }
        let Some(probe) = self.inflight.remove(&response.request_id.to_wire()) else {
            return;
        };
        let alive = matches!(response.outcome(), Some(FetchOutcome::Http(_)));
        let doc = response.json();
        let text = response.text().unwrap_or("");
        match probe.kind {
            ProbeKind::Liveness => {
                self.finished = self.finished.saturating_add(1);
                let ip = self.ip(probe.host);
                if let Some(miner) = classify(0, &ip, probe.host, text, &doc) {
                    let detail = miner.family == "harlo";
                    let host = miner.host;
                    self.upsert(miner);
                    if detail {
                        self.queue_detail(host);
                    }
                } else if alive {
                    self.queue.push_back(Probe {
                        host: probe.host,
                        kind: ProbeKind::Identify(1),
                    });
                } else {
                    self.queue_tcp(probe.host, false);
                    self.queue.push_back(Probe {
                        host: probe.host,
                        kind: ProbeKind::BitaxeRetry,
                    });
                }
            }
            ProbeKind::BitaxeRetry => {
                let ip = self.ip(probe.host);
                if let Some(miner) = classify(0, &ip, probe.host, text, &doc) {
                    let detail = miner.family == "harlo";
                    let host = miner.host;
                    self.upsert(miner);
                    if detail {
                        self.queue_detail(host);
                    }
                }
            }
            ProbeKind::Identify(step) => {
                if let Some(miner) = classify(step, &self.ip(probe.host), probe.host, text, &doc) {
                    let detail = miner.family == "harlo";
                    let host = miner.host;
                    self.upsert(miner);
                    if detail {
                        self.queue_detail(host);
                    }
                } else if !alive {
                    self.queue_tcp(probe.host, true);
                } else if step < 6 {
                    self.queue.push_back(Probe {
                        host: probe.host,
                        kind: ProbeKind::Identify(step + 1),
                    });
                } else {
                    self.queue_tcp(probe.host, true);
                }
            }
            ProbeKind::Login => {
                let token = doc.str("/token").unwrap_or_default();
                if !token.is_empty() {
                    if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == probe.host) {
                        miner.token = token;
                    }
                }
                self.queue.push_back(Probe {
                    host: probe.host,
                    kind: ProbeKind::Poll,
                });
            }
            ProbeKind::Poll => {
                let status = match response.outcome() {
                    Some(FetchOutcome::Http(code)) => Some(code),
                    _ => None,
                };
                if matches!(status, Some(401 | 403)) {
                    if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == probe.host) {
                        miner.token.clear();
                        if miner.hashrate_ths.is_none() {
                            note_miss(miner);
                        }
                        if !miner.user.is_empty() || !miner.pass.is_empty() {
                            self.queue.push_back(Probe {
                                host: probe.host,
                                kind: ProbeKind::Login,
                            });
                        }
                    }
                } else {
                let mut detail = false;
                if alive {
                    if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == probe.host) {
                        apply_http_poll(miner, text, &doc);
                        detail = miner.family == "harlo"
                            || (miner.family == "luckyMiner" && !has_reading(miner))
                            || (miner.family == "bitaxe"
                                && miner.hashrate_ths.is_none()
                                && miner.temp_c.is_none());
                    }
                } else if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == probe.host)
                {
                    note_miss(miner);
                }
                if detail {
                    self.queue_detail(probe.host);
                }
                }
            }
            ProbeKind::Detail => {
                let mut again = false;
                if alive {
                    if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == probe.host) {
                        if miner.family == "luckyMiner"
                            || (miner.family == "bitaxe" && (text.contains("hashrate_ghs") || text.contains("temp_core_1")))
                        {
                            apply_lucky_overview(miner, &doc);
                            if has_reading(miner) {
                                remember(miner);
                            }
                        } else if miner.family == "harlo" {
                            apply_pool(miner, &doc);
                        } else if miner.family == "braiins" {
                            let step = miner.detail_step;
                            if step == 0 {
                                apply_braiins_cooling(miner, &doc);
                            } else {
                                apply_braiins_boards(miner, &doc);
                            }
                            if has_reading(miner) {
                                remember(miner);
                            }
                            if step == 0 {
                                miner.detail_step = 1;
                                again = true;
                            }
                        }
                    }
                }
                if again {
                    self.queue_detail(probe.host);
                }
            }
        }
    }

    fn handle_socket(&mut self, socket: Socket, event: &SocketEvent<'_>) {
        let id = socket.0.to_wire();
        if self.sound_socket.is_some_and(|sound| sound.0.to_wire() == id) {
            match event {
                SocketEvent::Connected => {
                    let volume = self.sound_volume.to_string();
                    socket.write(format!("{volume}\n").as_bytes());
                    socket.close();
                }
                SocketEvent::Closed(_) => {
                    self.sound_socket = None;
                }
                SocketEvent::Data(_) => {}
            }
            return;
        }
        match event {
            SocketEvent::Connected => {
                if let Some(job) = self.sockets.get_mut(&id) {
                    job.connected = true;
                    let owned = if job.payload.is_empty() {
                        None
                    } else {
                        Some(job.payload.clone())
                    };
                    let command = if let Some(payload) = owned.as_deref() {
                        payload
                    } else if job.poll {
                        SUMMARY_CMD
                    } else {
                        VERSION_CMD
                    };
                    socket.write(command);
                }
            }
            SocketEvent::Data(data) => {
                if let Some(job) = self.sockets.get_mut(&id) {
                    job.buf.extend_from_slice(data);
                }
                self.finish_socket_buffer(id);
            }
            SocketEvent::Closed(_) => {
                let missed = self.sockets.get(&id).is_some_and(|job| {
                    job.poll && job.payload.is_empty() && !job.control && !frame_ready(&job.buf)
                });
                let host = self.sockets.get(&id).map(|job| job.host);
                self.finish_socket_buffer(id);
                if missed {
                    if let Some(host) = host {
                        if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == host) {
                            note_miss(miner);
                        }
                    }
                }
                self.sockets.remove(&id);
            }
        }
    }

    fn finish_socket_buffer(&mut self, id: u32) {
        let Some(job) = self.sockets.get(&id) else {
            return;
        };
        if !frame_ready(&job.buf) {
            return;
        }
        let host = job.host;
        let poll = job.poll;
        let control = job.control;
        let aux = !job.payload.is_empty();
        let asked = String::from_utf8_lossy(&job.payload).into_owned();
        let buf = job.buf.clone();
        let text = String::from_utf8_lossy(&buf).replace('\0', "");
        let doc = JsonDoc::parse(text.as_bytes());
        let mut follow = None;
        if control {
            self.notice = "Command sent".to_owned();
        } else if poll {
            if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == host) {
                if aux {
                    if asked.contains("temps") {
                        if text.contains("\"TEMPS\"") {
                            apply_board_temps(miner, &doc);
                        }
                        follow = Some(FANS_CMD);
                    } else if asked.contains("fans") {
                        if text.contains("\"FANS\"") {
                            apply_board_fans(miner, &doc);
                        }
                        if miner.family == "braiins" || miner.pool.is_empty() {
                            follow = Some(POOLS_CMD);
                        }
                    } else if text.contains("\"POOLS\"") {
                        apply_pool_list(miner, &doc);
                    } else {
                        apply_aux_poll(miner, &text);
                        if miner.family == "canaan" && miner.pool.is_empty() {
                            follow = Some(POOLS_CMD);
                        }
                        if miner.family == "bitmainZEC" {
                            follow = Some(POOLS_CMD);
                        }
                    }
                } else {
                    apply_cgminer_poll(miner, &text, &doc);
                    follow = match miner.family {
                        "canaan" => Some(ESTATS_CMD),
                        "bitmainZEC" => Some(STATS_CMD),
                        "braiins" => Some(TEMPS_CMD),
                        "luxos" | "cgminer" => Some(POOLS_CMD),
                        _ => None,
                    };
                }
            }
        } else if doc.is_valid() && !self.miners.iter().any(|miner| miner.host == host) {
            let ip = self.ip(host);
            self.upsert(classify_cgminer(&ip, host, &text, &doc));
        }
        if let Some(job) = self.sockets.remove(&id) {
            job.socket.close();
        }
        if let Some(command) = follow {
            self.open_aux_socket(host, command);
        }
    }

    fn reap_sockets(&mut self) {
        let now = TICK.get();
        let stale: Vec<u32> = self
            .sockets
            .iter()
            .filter(|(_, job)| {
                let stale = now.saturating_sub(job.started) >= SOCKET_TICKS;
                stale && (!job.connected || job.buf.is_empty())
            })
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            if let Some(job) = self.sockets.remove(&id) {
                if job.poll && job.payload.is_empty() && !job.control {
                    if let Some(miner) = self.miners.iter_mut().find(|miner| miner.host == job.host) {
                        note_miss(miner);
                    }
                }
                job.socket.close();
            }
        }
    }

    fn open_aux_socket(&mut self, host: u8, command: &'static [u8]) {
        if self.sockets.len() >= MAX_SOCKETS {
            return;
        }
        if self.sockets.values().any(|job| job.host == host && !job.payload.is_empty()) {
            return;
        }
        let ip = self.ip(host);
        let Some(socket) = socket::tcp_connect(&ip, 4028, on_socket) else {
            return;
        };
        self.sockets.insert(
            socket.0.to_wire(),
            SocketJob {
                host,
                started: TICK.get(),
                connected: false,
                poll: true,
                control: false,
                payload: command.to_vec(),
                buf: Vec::new(),
                socket,
            },
        );
    }

    /// One read-only status round per miner. Pool data is fetched with GET or
    /// the CGMiner `pools` command and is never written back.
    fn enqueue_polls(&mut self) {
        let jobs: Vec<(u8, &'static str, bool, bool)> = self
            .miners
            .iter()
            .map(|miner| {
                let authed = miner.family == "braiins"
                    || (miner.family == "vnish" && !miner.pass.is_empty());
                (miner.host, miner.family, authed, !miner.token.is_empty())
            })
            .collect();
        for (host, family, authed, has_token) in jobs {
            let tcp = matches!(family, "cgminer" | "luxos" | "canaan" | "bitmainZEC" | "braiins");
            if tcp {
                self.open_poll_socket(host);
            }
            if authed && !has_token {
                self.queue.push_back(Probe {
                    host,
                    kind: ProbeKind::Login,
                });
            } else if !tcp || (authed && has_token) {
                self.queue.push_back(Probe {
                    host,
                    kind: ProbeKind::Poll,
                });
            }
        }
    }

    fn open_poll_socket(&mut self, host: u8) {
        if self.sockets.len() >= MAX_SOCKETS {
            self.tcp_queue.push_back(host);
            return;
        }
        let ip = self.ip(host);
        let Some(socket) = socket::tcp_connect(&ip, 4028, on_socket) else {
            return;
        };
        self.sockets.insert(
            socket.0.to_wire(),
            SocketJob {
                host,
                started: TICK.get(),
                connected: false,
                poll: true,
                control: false,
                payload: Vec::new(),
                buf: Vec::new(),
                socket,
            },
        );
    }

    fn queue_tcp(&mut self, host: u8, urgent: bool) {
        if self.miners.iter().any(|miner| miner.host == host) {
            return;
        }
        if self.tcp_queue.contains(&host) || self.sockets.values().any(|job| job.host == host) {
            return;
        }
        if urgent {
            self.tcp_queue.push_front(host);
        } else {
            self.tcp_queue.push_back(host);
        }
    }

    fn upsert(&mut self, miner: Miner) {
        let Some(existing) = self.miners.iter_mut().find(|item| item.ip == miner.ip) else {
            return;
        };
        let name = existing.name.clone();
        let mac = existing.mac.clone();
        let tune = existing.tune.clone();
        let user = existing.user.clone();
        let pass = existing.pass.clone();
        let token = existing.token.clone();
        let family = existing.family;
        let shares_accepted = existing.shares_accepted;
        let shares_seen = existing.shares_seen;
        let share_locked = existing.share_locked;
        *existing = miner;
        if !name.is_empty() {
            existing.name = name;
        }
        if !mac.is_empty() {
            existing.mac = mac;
        }
        if !tune.is_empty() {
            existing.tune = tune;
        }
        if !user.is_empty() {
            existing.user = user;
        }
        if !pass.is_empty() {
            existing.pass = pass;
        }
        if !token.is_empty() {
            existing.token = token;
        }
        if family != "miner" {
            existing.family = family;
        }
        if share_locked {
            existing.shares_accepted = shares_accepted;
            existing.shares_seen = shares_seen;
            existing.share_locked = true;
        }
        self.tcp_queue.retain(|host| self.miners.iter().all(|miner| miner.host != *host));
    }

    fn handle_click(&mut self, id: &str) -> bool {
        if self.celebration.is_some() {
            if id == "best-awesome" {
                self.celebration = None;
                self.end_celebration_leds();
                self.promote_best();
                if self.celebration.is_some() {
                    self.drive_celebration_leds(5_000);
                }
            }
            return true;
        }
        if id == "saver-wake" {
            self.saver_on = false;
            self.idle_ms = 0;
            return true;
        }
        if id == "rescan" {
            self.apply_inventory();
            return true;
        }
        if id == "view-fleet" {
            self.neural = false;
            self.summary = false;
            self.neural_hint_ms = 0;
            return true;
        }
        if id == "view-neural" {
            let entering = !self.neural || self.summary;
            self.neural = true;
            self.summary = false;
            self.settings = false;
            if entering {
                self.neural_hint_ms = 2_450;
            }
            return true;
        }
        if id == "view-summary" {
            self.summary = true;
            self.neural = false;
            self.settings = false;
            self.neural_hint_ms = 0;
            return true;
        }
        if let Some(index) = id.strip_prefix("sum-") {
            if let Ok(index) = index.parse::<usize>() {
                if index < self.summary_cols.len() {
                    self.cycle_summary_col(index);
                }
            }
            return true;
        }
        if let Some(index) = id.strip_prefix("colslot-") {
            if let Ok(index) = index.parse::<u8>() {
                if index < 4 {
                    self.summary_edit = index;
                }
            }
            return true;
        }
        if let Some(col_id) = id.strip_prefix("colopt-") {
            if let Some(col) = summary_col_from_id(col_id) {
                self.assign_summary_col(self.summary_edit as usize, col);
            }
            return true;
        }
        if id == "open-settings" {
            self.settings = true;
            self.settings_page = 0;
            self.tailscale_wait_ms = 8_000;
            self.cancel_work();
            return true;
        }
        if id == "settings-menu" {
            self.settings_page = 0;
            return true;
        }
        if id == "settings-display" {
            self.settings_page = 1;
            return true;
        }
        if id == "settings-rgb" {
            self.settings_page = 2;
            return true;
        }
        if id == "settings-layout" {
            self.settings_page = 3;
            return true;
        }
        if id == "settings-alerts" {
            self.settings_page = 4;
            return true;
        }
        if id == "settings-refresh" {
            self.settings_page = 5;
            return true;
        }
        if id == "settings-support" {
            self.settings_page = 6;
            return true;
        }
        if id == "tailscale-open" {
            self.tailscale_open = true;
            self.tailscale_wait_ms = 8_000;
            return true;
        }
        if id == "tailscale-close" {
            self.tailscale_open = false;
            return true;
        }
        if id == "tailscale-stop" {
            self.tailscale_command("stop");
            return true;
        }
        if id == "tailscale-start" {
            self.tailscale_command("start");
            return true;
        }
        if id == "tailscale-restart" {
            self.tailscale_command("restart");
            return true;
        }
        if id == "tailscale-reset" {
            self.tailscale_command("reset");
            return true;
        }
        if self.handle_alarm(id) {
            return true;
        }
        if id == "close-settings" {
            self.settings = false;
            self.settings_page = 0;
            self.tailscale_open = false;
            self.since_poll = self.poll_ms;
            return true;
        }
        if id == "poll-now" {
            self.since_poll = self.poll_ms;
            return true;
        }
        if let Some(seconds) = id.strip_prefix("rate-") {
            if let Ok(seconds) = seconds.parse::<u32>() {
                self.poll_ms = seconds.saturating_mul(1_000).clamp(1_000, 60_000);
                self.store_poll();
            }
            return true;
        }
        if id == "names-on" {
            self.label_mode = 0;
            self.store_saver();
            return true;
        }
        if id == "names-hash" {
            self.label_mode = 1;
            self.store_saver();
            return true;
        }
        if id == "names-off" {
            self.label_mode = 2;
            self.store_saver();
            return true;
        }
        if id == "fleet-hash" {
            self.fleet_groups = false;
            self.fleet_row = 0;
            self.store_saver();
            return true;
        }
        if id == "fleet-groups" {
            self.fleet_groups = true;
            self.fleet_row = 0;
            self.store_saver();
            return true;
        }
        if id == "pulse-on" {
            self.share_pulse = true;
            self.touch_controls();
            return true;
        }
        if id == "pulse-off" {
            self.share_pulse = false;
            self.touch_controls();
            return true;
        }
        if id == "pulse-test" {
            let ip = self
                .miners
                .iter()
                .find(|miner| miner.reachable)
                .or_else(|| self.miners.first())
                .map(|miner| miner.ip.clone())
                .unwrap_or_default();
            self.flash_share(&ip);
            return true;
        }
        if id == "sound-play" {
            self.play_share_sound();
            return true;
        }
        if id == "preview-best" || id == "preview-block" {
            self.preview_celebration(if id == "preview-block" { 1 } else { 0 });
            return true;
        }
        if let Some(volume) = id.strip_prefix("vol-") {
            if let Ok(volume) = volume.parse::<u8>() {
                self.sound_volume = snap_volume(volume);
                self.store_saver();
            }
            return true;
        }
        if id == "temp-f" {
            self.weather_f = true;
            self.store_saver();
            return true;
        }
        if id == "temp-c" {
            self.weather_f = false;
            self.store_saver();
            return true;
        }
        if id == "saver-clock" {
            self.saver_weather = false;
            self.store_saver();
            return true;
        }
        if id == "outbound-on" || id == "outbound-off" {
            self.set_outbound(id == "outbound-on");
            return true;
        }
        if id == "saver-weather" {
            self.saver_weather = true;
            self.store_saver();
            self.weather.fetched_at = 0;
            return true;
        }
        if let Some(ms) = id.strip_prefix("saver-") {
            if let Ok(ms) = ms.parse::<u32>() {
                self.saver_ms = ms;
                self.idle_ms = 0;
                self.saver_on = false;
                self.store_saver();
            }
            return true;
        }
        if id == "place-edit" {
            self.place_edit = true;
            self.place_shift = self.place.is_empty();
            self.places.clear();
            self.search_places();
            return true;
        }
        if let Some(index) = id.strip_prefix("hit-") {
            if let Ok(index) = index.parse::<usize>() {
                if let Some(hit) = self.places.get(index).cloned() {
                    self.choose_place(hit);
                }
            }
            return true;
        }
        if id == "k-cancel" {
            if self.alarm_naming {
                self.alarm_naming = false;
                return true;
            }
            self.place_edit = false;
            self.places.clear();
            self.suggest_fetch = None;
            self.place = bmc_wasm_sdk::kv::get_string("place").unwrap_or_default();
            self.place_shift = self.place.is_empty();
            return true;
        }
        if id == "k-done" {
            if self.alarm_naming {
                self.alarm_naming = false;
                return true;
            }
            self.place_edit = false;
            self.weather = Weather::default();
            self.weather_geo = true;
            self.weather_fetch = None;
            self.place_local = true;
            self.store_saver();
            return true;
        }
        if id == "k-back" {
            if self.alarm_naming {
                self.alarm_name.pop();
                return true;
            }
            self.place.pop();
            self.place_shift = self.place.is_empty() || self.place.ends_with(' ');
            self.search_places();
            return true;
        }
        if id == "k-space" {
            if self.alarm_naming {
                if self.alarm_name.len() < 24 && !self.alarm_name.is_empty() && !self.alarm_name.ends_with(' ') {
                    self.alarm_name.push(' ');
                }
                return true;
            }
            if self.place.len() < 32 && !self.place.is_empty() && !self.place.ends_with(' ') {
                self.place.push(' ');
                self.place_shift = true;
                self.search_places();
            }
            return true;
        }
        if let Some(letter) = id.strip_prefix("k-") {
            if self.alarm_naming {
                if letter.len() == 1 && self.alarm_name.len() < 24 {
                    let ch = letter.chars().next().unwrap_or('a');
                    let ch = if self.alarm_name.is_empty() || self.alarm_name.ends_with(' ') {
                        ch.to_ascii_uppercase()
                    } else {
                        ch
                    };
                    self.alarm_name.push(ch);
                }
                return true;
            }
            if letter.len() == 1 && self.place.len() < 32 {
                let mut ch = letter.chars().next().unwrap_or('a');
                if self.place_shift {
                    ch = ch.to_ascii_uppercase();
                }
                self.place.push(ch);
                self.place_shift = false;
                self.search_places();
            }
            return true;
        }
        if id == "back" {
            if self.chart_open {
                self.chart_open = false;
                return true;
            }
            self.selected = None;
            self.notice.clear();
            return true;
        }
        if id == "chart-open" && self.selected.is_some() {
            self.chart_open = true;
            self.flush_charts(true);
            return true;
        }
        if let Some(raw) = id.strip_prefix("span-") {
            if let Ok(index) = raw.parse::<u8>() {
                if (index as usize) < CHART_SPANS.len() {
                    self.chart_span = index;
                    return true;
                }
            }
        }
        if let Some(ip) = id.strip_prefix("miner-") {
            self.chart_open = false;
            self.selected = Some(ip.to_owned());
            self.notice.clear();
            return true;
        }
        if self.apply_led(id) {
            return true;
        }
        let Some(miner) = self
            .selected
            .clone()
            .and_then(|ip| self.miners.iter().find(|miner| miner.ip == ip).cloned())
        else {
            return false;
        };
        if id == "fan-manual" || id.starts_with("fan-") {
            if let Some(stored) = self.miners.iter_mut().find(|item| item.ip == miner.ip) {
                if id == "fan-manual" {
                    stored.fan_manual = true;
                    self.notice = "Pick a fan speed".to_owned();
                    return true;
                }
                stored.fan_manual = id != "fan-auto";
            }
        }
        self.apply_miner_action(&miner, id)
    }

    fn apply_led(&mut self, id: &str) -> bool {
        if id == "led-rainbow" {
            self.led_rainbow = true;
            self.led_hold = true;
            self.led_label = "Rainbow".to_owned();
            if self.celebration.is_some() {
                return true;
            }
            self.rainbow_ms = 1_200;
            self.led_refresh_ms = 0;
            self.touch_controls();
            return true;
        }
        let known = matches!(
            id,
            "led-emerald"
                | "led-white"
                | "led-amber"
                | "led-red"
                | "led-blue"
                | "led-purple"
                | "led-off"
                | "led-solid"
                | "led-breathe"
                | "led-chase"
                | "led-scan"
                | "led-snake"
                | "led-rider"
        );
        if !known {
            return false;
        }
        self.led_rainbow = false;
        match id {
            "led-emerald" => {
                self.led_rgb = (0, 204, 102);
                self.led_effect = LedEffect::Solid;
            }
            "led-white" => {
                self.led_rgb = (255, 255, 255);
                self.led_effect = LedEffect::Solid;
            }
            "led-amber" => {
                self.led_rgb = (255, 196, 0);
                self.led_effect = LedEffect::Solid;
            }
            "led-red" => {
                self.led_rgb = (255, 32, 32);
                self.led_effect = LedEffect::Solid;
            }
            "led-blue" => {
                self.led_rgb = (0, 90, 255);
                self.led_effect = LedEffect::Solid;
            }
            "led-purple" => {
                self.led_rgb = (180, 40, 255);
                self.led_effect = LedEffect::Solid;
            }
            "led-off" => {
                self.led_hold = false;
                self.led_label = "Off".to_owned();
                if self.celebration.is_none() {
                    led::stop();
                }
                self.touch_controls();
                return true;
            }
            "led-solid" => self.led_effect = LedEffect::Solid,
            "led-breathe" => self.led_effect = LedEffect::Breathe,
            "led-chase" => self.led_effect = LedEffect::Chase,
            "led-scan" => self.led_effect = LedEffect::Scan,
            "led-snake" => self.led_effect = LedEffect::Snake,
            "led-rider" => self.led_effect = LedEffect::KnightRider,
            _ => return false,
        }
        self.paint_led();
        self.touch_controls();
        true
    }

    fn handle_alarm(&mut self, id: &str) -> bool {
        if id == "alarm-open" {
            self.alarm_open = true;
            self.alarm_naming = false;
            self.notice.clear();
            return true;
        }
        if !self.alarm_open && id != "alarm-add" {
            return false;
        }
        match id {
            "alarm-close" => {
                self.alarm_open = false;
                self.alarm_naming = false;
            }
            "alarm-name" => self.alarm_naming = true,
            "alarm-h-down" => self.alarm_hour = if self.alarm_hour <= 1 { 12 } else { self.alarm_hour - 1 },
            "alarm-h-up" => self.alarm_hour = if self.alarm_hour >= 12 { 1 } else { self.alarm_hour + 1 },
            "alarm-m-down" => self.alarm_minute = if self.alarm_minute == 0 { 55 } else { self.alarm_minute.saturating_sub(5) },
            "alarm-m-up" => self.alarm_minute = if self.alarm_minute >= 55 { 0 } else { self.alarm_minute + 5 },
            "alarm-ampm" => self.alarm_pm = !self.alarm_pm,
            "alarm-sound-prev" => {
                let count = ALARM_SOUNDS.len() as u8;
                self.alarm_sound = if self.alarm_sound == 0 { count - 1 } else { self.alarm_sound - 1 };
            }
            "alarm-sound-next" => {
                let count = ALARM_SOUNDS.len() as u8;
                self.alarm_sound = (self.alarm_sound + 1) % count;
            }
            "alarm-play" => self.deck_play_alarm(),
            "alarm-snooze-on" => self.alarm_snooze = true,
            "alarm-snooze-off" => self.alarm_snooze = false,
            "alarm-limit-1" => self.alarm_limit = 1,
            "alarm-limit-3" => self.alarm_limit = 2,
            "alarm-limit-5" => self.alarm_limit = 3,
            "alarm-add" => self.deck_add_alarm(),
            _ => {
                if let Some(day) = id.strip_prefix("alarm-day-") {
                    if let Ok(day) = day.parse::<u8>() {
                        if day < 7 {
                            self.alarm_days ^= 1 << day;
                        }
                    }
                    return true;
                }
                return false;
            }
        }
        true
    }

    fn deck_add_alarm(&mut self) {
        if self.alarm_days == 0 {
            self.notice = "Pick at least one day".to_owned();
            return;
        }
        self.alarm_after = 1;
        self.deck_flush_alarm();
    }

    fn deck_play_alarm(&mut self) {
        self.alarm_after = 2;
        self.deck_flush_alarm();
    }

    fn deck_flush_alarm(&mut self) {
        if self.deck_fetch.is_some() {
            return;
        }
        if self.deck_token.is_empty() {
            self.deck_login();
            return;
        }
        let after = self.alarm_after;
        self.alarm_after = 0;
        if after == 1 {
            self.notice = "Saving alarm".to_owned();
            self.deck_send(
                4,
                "/braiins.bmc.web.AlarmService/AddAlarm",
                grpc_frame(&alarm_request(self)),
                true,
            );
        } else if after == 2 {
            let sound = ALARM_SOUNDS
                .get(self.alarm_sound as usize)
                .map(|sound| sound.0)
                .unwrap_or(ALARM_SOUNDS[0].0);
            self.deck_send(
                5,
                "/braiins.bmc.web.ConfigurationService/PlaySound",
                grpc_frame(&proto_string_field(1, sound)),
                true,
            );
        }
    }

    fn release_shares(&mut self) {
        if self.phase != Phase::Live {
            return;
        }
        if !self.queue.is_empty() || !self.inflight.is_empty() || !self.sockets.is_empty() || !self.tcp_queue.is_empty() {
            return;
        }
        let mut ips = Vec::new();
        for miner in &mut self.miners {
            let Some(now) = miner.shares_accepted else {
                continue;
            };
            if miner.shares_seen.is_some_and(|before| now > before) && !miner.ip.is_empty() {
                ips.push(miner.ip.clone());
            }
            miner.shares_seen = Some(now);
        }
        if !ips.is_empty() {
            self.flash_shares(ips);
        }
    }

    fn flash_share(&mut self, ip: &str) {
        if ip.is_empty() {
            return;
        }
        self.flash_shares(vec![ip.to_owned()]);
    }

    fn flash_shares(&mut self, ips: Vec<String>) {
        if ips.is_empty() {
            return;
        }
        self.share_ips = ips;
        self.share_flash_ms = SHARE_FLASH_MS;
        if self.share_pulse && self.celebration.is_none() {
            self.pulse_share_led();
        }
        if self.share_sound {
            self.play_share_sound();
        }
    }

    fn cycle_summary_col(&mut self, index: usize) {
        let current = self.summary_cols[index];
        let next = (current + 1) % SUMMARY_COL_COUNT;
        if let Some(other) = self.summary_cols.iter().position(|col| *col == next) {
            self.summary_cols[other] = current;
        }
        self.summary_cols[index] = next;
        self.store_saver();
    }

    fn assign_summary_col(&mut self, index: usize, next: u8) {
        if index >= self.summary_cols.len() {
            return;
        }
        if let Some(other) = self.summary_cols.iter().position(|col| *col == next) {
            if other != index {
                self.summary_cols[other] = self.summary_cols[index];
            }
        }
        self.summary_cols[index] = next;
        self.store_saver();
    }

    fn preview_celebration(&mut self, kind: u8) {
        let sample = self.miners.iter().find(|miner| miner.reachable).or_else(|| self.miners.first());
        let name = sample
            .map(|miner| {
                if miner.name.trim().is_empty() {
                    miner.ip.clone()
                } else {
                    miner.name.clone()
                }
            })
            .unwrap_or_else(|| "Your miner".to_owned());
        let pool = sample
            .map(|miner| miner.pool.clone())
            .filter(|pool| !pool.is_empty())
            .unwrap_or_else(|| "stratum+tcp://public-pool.io:3333".to_owned());
        let difficulty = sample.and_then(|miner| miner.best_diff).unwrap_or(1_590_000_000.0);
        self.celebration = Some(BestPopup {
            name,
            pool,
            difficulty,
            graphic: 0,
            kind,
        });
        self.celebration_led = 0;
        self.rainbow_ms = 5_000;
        self.drive_celebration_leds(5_000);
    }

    fn promote_best(&mut self) {
        if self.celebration.is_none() {
            self.celebration = BEST_QUEUE.with(|queue| queue.borrow_mut().pop_front());
        }
        let Some(name) = self
            .celebration
            .as_ref()
            .filter(|popup| popup.pool.trim().is_empty())
            .map(|popup| popup.name.clone())
        else {
            return;
        };
        let pool = self
            .miners
            .iter()
            .find(|miner| miner.name == name || miner.ip == name)
            .map(|miner| miner.pool.clone())
            .unwrap_or_default();
        if pool.trim().is_empty() {
            return;
        }
        if let Some(popup) = self.celebration.as_mut() {
            popup.pool = pool;
        }
    }

    fn play_share_sound(&mut self) {
        if self.sound_volume == 0 {
            return;
        }
        if let Some(socket) = self.sound_socket {
            if TICK.get().saturating_sub(self.sound_started) < SOCKET_TICKS {
                return;
            }
            socket.close();
            self.sound_socket = None;
        }
        let Some(socket) = socket::tcp_connect("127.0.0.1", COIN_PORT, on_socket) else {
            return;
        };
        self.sound_socket = Some(socket);
        self.sound_started = TICK.get();
    }

    fn pulse_share_led(&mut self) {
        if self.celebration.is_some() {
            return;
        }
        let color = Color::from_rgba(255, 159, 10, 255);
        led::stop();
        led::set_effect(LedEffect::Breathe, color, 280, Some(SHARE_FLASH_MS));
        led::set_effect_global(LedEffect::Breathe, color, 280, Some(SHARE_FLASH_MS));
    }

    fn drive_celebration_leds(&mut self, delta_ms: u32) {
        let Some(kind) = self.celebration.as_ref().map(|popup| popup.kind) else {
            if self.celebration_led != 0 {
                self.end_celebration_leds();
            }
            return;
        };
        self.rainbow_ms = self.rainbow_ms.saturating_add(delta_ms);
        // The strip only honors a timed effect, and a repeat while one is running
        // does not extend it. Refresh before that window ends, and only stop on Awesome.
        if self.celebration_led != 0 && self.rainbow_ms < 1_200 {
            return;
        }
        self.rainbow_ms = 0;
        let color = if kind == 1 {
            self.rainbow_hue = (self.rainbow_hue + 28) % 360;
            let (red, green, blue) = hue_rgb(self.rainbow_hue);
            Color::from_rgba(red, green, blue, 255)
        } else {
            Color::from_rgba(0, 255, 70, 255)
        };
        self.sweep_leds(color);
        self.celebration_led = if kind == 1 { 2 } else { 1 };
    }

    fn sweep_leds(&mut self, color: Color) {
        led::stop();
        led::set_effect(LedEffect::KnightRider, color, 280, Some(3_500));
        led::set_effect_global(LedEffect::KnightRider, color, 280, Some(3_500));
    }

    fn end_celebration_leds(&mut self) {
        self.celebration_led = 0;
        self.rainbow_ms = 0;
        if self.led_label == "Off" {
            led::stop();
        } else if self.led_rainbow {
            self.rainbow_ms = 1_200;
            self.led_refresh_ms = 0;
        } else {
            self.arm_led();
        }
    }

    fn paint_led(&mut self) {
        let (r, g, b) = self.led_rgb;
        self.led_period = match self.led_effect {
            LedEffect::Solid => 1_000,
            LedEffect::Breathe => 4_800,
            LedEffect::Chase => 700,
            LedEffect::Scan => 2_200,
            LedEffect::Snake => 4_200,
            LedEffect::KnightRider => 800,
        };
        self.led_hold = true;
        self.led_refresh_ms = 0;
        self.arm_led();
        self.led_label = format!("{}  #{r:02X}{g:02X}{b:02X}", led_effect_name(self.led_effect));
    }

    fn arm_led(&mut self) {
        // Best diff and block found own the strip until Awesome.
        if self.celebration.is_some() {
            return;
        }
        let (r, g, b) = self.led_rgb;
        let color = Color::from_rgba(r, g, b, 255);
        // A running effect ignores a new color or mode until it is stopped.
        led::stop();
        led::set_effect(self.led_effect, color, self.led_period, Some(LED_RUN_MS));
        led::set_effect_global(self.led_effect, color, self.led_period, Some(LED_RUN_MS));
    }

    fn drive_user_leds(&mut self, delta_ms: u32) {
        if self.celebration.is_some() || !self.led_hold {
            return;
        }
        if self.share_flash_ms > 0 {
            if self.led_rainbow {
                self.rainbow_ms = 1_200;
                self.led_refresh_ms = 0;
            } else {
                self.led_refresh_ms = LED_REARM_MS;
            }
            return;
        }
        if self.led_rainbow {
            self.drive_rainbow_flash(delta_ms);
            return;
        }
        self.led_refresh_ms = self.led_refresh_ms.saturating_add(delta_ms);
        if self.led_refresh_ms < LED_REARM_MS {
            return;
        }
        self.led_refresh_ms = 0;
        self.arm_led();
    }

    fn drive_rainbow_flash(&mut self, delta_ms: u32) {
        if self.celebration.is_some() {
            return;
        }
        self.rainbow_ms = self.rainbow_ms.saturating_add(delta_ms);
        // Same sweep as a block found: Night Rider at 280ms, new hue every 1.2s.
        if self.led_refresh_ms != 0 && self.rainbow_ms < 1_200 {
            return;
        }
        self.rainbow_ms = 0;
        self.led_refresh_ms = 1;
        self.rainbow_hue = (self.rainbow_hue + 28) % 360;
        let (red, green, blue) = hue_rgb(self.rainbow_hue);
        self.sweep_leds(Color::from_rgba(red, green, blue, 255));
    }

    fn apply_miner_action(&mut self, miner: &Miner, id: &str) -> bool {
        let ip = &miner.ip;
        match id {
            "fan-auto" | "fan-40" | "fan-60" | "fan-80" | "fan-100" => self.apply_fan(miner, id),
            "mode-0" | "mode-1" | "mode-2" => self.apply_work_mode(miner, id),
            "act-restart" => match miner.family {
                "bitaxe" | "luckyMiner" => {
                    self.send_post(&format!("http://{ip}/api/system/restart"), "Restarting miner");
                }
                "harlo" => self.send_post(&format!("http://{ip}/api/v1/reboot"), "Restarting miner"),
                "braiins" => self.send_braiins(miner, "/api/v1/actions/restart", "", "Restarting miner"),
                "canaan" => self.send_tcp_raw(miner.host, "ascset|0,reboot,0", "Restarting miner"),
                "bitmainZEC" => self.start_zec_reboot(miner),
                "luxos" | "cgminer" => {
                    self.send_tcp_command(miner.host, "{\"command\":\"restart\"}", "Restarting miner");
                }
                _ => self.notice = "Restart is not available for this miner.".to_owned(),
            },
            "act-pause" => match miner.family {
                "bitaxe" | "luckyMiner" => {
                    self.send_post(&format!("http://{ip}/api/system/pause"), "Pausing miner");
                }
                "harlo" => self.send_post_json(
                    &format!("http://{ip}/api/v1/mining"),
                    "{\"enabled\":false}",
                    "Pausing miner",
                ),
                "braiins" => self.send_braiins(miner, "/api/v1/actions/pause", "", "Pausing miner"),
                _ => self.notice = "Pause is not available for this miner.".to_owned(),
            },
            "act-resume" => match miner.family {
                "bitaxe" | "luckyMiner" => {
                    self.send_post(&format!("http://{ip}/api/system/resume"), "Resuming miner");
                }
                "harlo" => self.send_post_json(
                    &format!("http://{ip}/api/v1/mining"),
                    "{\"enabled\":true}",
                    "Resuming miner",
                ),
                "braiins" => self.send_braiins(miner, "/api/v1/actions/resume", "", "Resuming miner"),
                _ => self.notice = "Resume is not available for this miner.".to_owned(),
            },
            _ if id.starts_with("tune-") => self.apply_braiins_tune(miner, id),
            _ => return false,
        }
        true
    }

    fn start_zec_fan(&mut self, miner: &Miner, speed: &str) {
        if miner.pass.is_empty() {
            self.notice = "Send the Bitmain web login from HashWatcher, then set the fan.".to_owned();
            return;
        }
        let manual = speed != "auto";
        let percent = if manual { speed.parse::<i32>().unwrap_or(80) } else { 100 };
        self.notice = if manual {
            format!("Setting fan to {percent}%")
        } else {
            "Setting fan to auto".to_owned()
        };
        self.zec_fan = Some(ZecFanJob {
            ip: miner.ip.clone(),
            user: if miner.user.is_empty() { "root".to_owned() } else { miner.user.clone() },
            pass: miner.pass.clone(),
            manual,
            percent,
            reboot: false,
            step: 0,
            realm: String::new(),
            nonce: String::new(),
            qop: false,
            conf: String::new(),
        });
        self.zec_call("GET", "/cgi-bin/get_miner_conf.cgi", "", false);
    }

    fn start_zec_reboot(&mut self, miner: &Miner) {
        if miner.pass.is_empty() {
            self.notice = "Send the Bitmain web login from HashWatcher, then restart.".to_owned();
            return;
        }
        self.notice = "Restarting miner".to_owned();
        self.zec_fan = Some(ZecFanJob {
            ip: miner.ip.clone(),
            user: if miner.user.is_empty() { "root".to_owned() } else { miner.user.clone() },
            pass: miner.pass.clone(),
            manual: false,
            percent: 0,
            reboot: true,
            step: 0,
            realm: String::new(),
            nonce: String::new(),
            qop: false,
            conf: String::new(),
        });
        self.zec_call("GET", "/cgi-bin/reboot.cgi", "", false);
    }

    fn zec_call(&mut self, method: &str, path: &str, body: &str, authed: bool) {
        let Some(job) = self.zec_fan.as_ref() else {
            return;
        };
        let url = format!("http://{}{path}", job.ip);
        let mut headers = String::from("Content-Type: application/x-www-form-urlencoded; charset=utf-8");
        if authed {
            let auth = digest_header(&job.user, &job.pass, &job.realm, &job.nonce, job.qop, method, path);
            headers.push('\n');
            headers.push_str(&auth);
        }
        let request = if method == "POST" {
            FetchRequest::post(&url).headers(&headers).body(body.as_bytes())
        } else {
            FetchRequest::get(&url).headers(&headers)
        };
        let Some(id) = request.timeout(Duration::from_millis(4_000)).send(on_fetch) else {
            self.notice = "Could not reach the Bitmain web login.".to_owned();
            self.zec_fan = None;
            return;
        };
        self.zec_fetch = Some(id.to_wire());
    }

    fn finish_zec(&mut self, response: &FetchResponse) {
        let Some(step) = self.zec_fan.as_ref().map(|job| job.step) else {
            return;
        };
        let reboot = self.zec_fan.as_ref().is_some_and(|job| job.reboot);
        if reboot {
            match step {
                0 => {
                    let challenge = response.header("www-authenticate").unwrap_or_default();
                    let accepted = response.status == 401
                        && self
                            .zec_fan
                            .as_mut()
                            .is_some_and(|job| read_digest_challenge(&challenge, job));
                    if !accepted {
                        self.zec_fan = None;
                        self.notice = "Bitmain did not accept a restart login.".to_owned();
                        return;
                    }
                    if let Some(job) = self.zec_fan.as_mut() {
                        job.step = 1;
                    }
                    self.zec_call("GET", "/cgi-bin/reboot.cgi", "", true);
                }
                _ => {
                    let ok = (200..300).contains(&response.status);
                    self.zec_fan = None;
                    self.notice = if ok {
                        "Restarting miner".to_owned()
                    } else {
                        "Bitmain did not confirm the restart.".to_owned()
                    };
                }
            }
            return;
        }
        match step {
            0 => {
                let challenge = response.header("www-authenticate").unwrap_or_default();
                let accepted = response.status == 401
                    && self
                        .zec_fan
                        .as_mut()
                        .is_some_and(|job| read_digest_challenge(&challenge, job));
                if !accepted {
                    self.zec_fan = None;
                    self.notice = "Bitmain did not accept a fan login.".to_owned();
                    return;
                }
                if let Some(job) = self.zec_fan.as_mut() {
                    job.step = 1;
                }
                self.zec_call("GET", "/cgi-bin/get_miner_conf.cgi", "", true);
            }
            1 => {
                if !(200..300).contains(&response.status) {
                    self.zec_fan = None;
                    self.notice = "Bitmain web login was rejected.".to_owned();
                    return;
                }
                let body = response.text().unwrap_or("").to_owned();
                if let Some(job) = self.zec_fan.as_mut() {
                    job.conf = body;
                    job.step = 2;
                }
                self.zec_call("POST", "/cgi-bin/set_miner_conf.cgi", "", false);
            }
            2 => {
                let challenge = response.header("www-authenticate").unwrap_or_default();
                let accepted = response.status == 401
                    && self
                        .zec_fan
                        .as_mut()
                        .is_some_and(|job| read_digest_challenge(&challenge, job));
                if !accepted {
                    self.zec_fan = None;
                    self.notice = "Bitmain refused the fan change.".to_owned();
                    return;
                }
                let form = self.zec_fan.as_ref().and_then(|job| zec_fan_form(&job.conf, job.manual, job.percent));
                let Some(form) = form else {
                    self.zec_fan = None;
                    self.notice = "Bitmain pool settings look incomplete, so the fan was left alone.".to_owned();
                    return;
                };
                if let Some(job) = self.zec_fan.as_mut() {
                    job.step = 3;
                }
                self.zec_call("POST", "/cgi-bin/set_miner_conf.cgi", &form, true);
            }
            _ => {
                let ok = (200..300).contains(&response.status);
                let text = response.text().unwrap_or("").trim().to_ascii_lowercase();
                self.zec_fan = None;
                self.notice = if ok && (text.is_empty() || text.contains("ok")) {
                    "Fan setting sent. The miner restarts cgminer.".to_owned()
                } else {
                    "Bitmain did not confirm the fan change.".to_owned()
                };
            }
        }
    }

    fn apply_fan(&mut self, miner: &Miner, id: &str) {
        let ip = &miner.ip;
        let speed = id.trim_start_matches("fan-");
        match miner.family {
            "bitaxe" | "luckyMiner" => {
                let nerd = miner.name.to_ascii_lowercase().contains("nerd")
                    || miner.model.to_ascii_lowercase().contains("nerd");
                let auto_mode = if nerd { 2 } else { 1 };
                if speed == "auto" {
                    self.send_patch(
                        &format!("http://{ip}/api/system"),
                        &format!("{{\"autofanspeed\":{auto_mode}}}"),
                        "Setting fan to auto",
                    );
                } else {
                    let body = format!("{{\"manualFanSpeed\":{speed},\"autofanspeed\":0}}");
                    self.send_patch(
                        &format!("http://{ip}/api/system"),
                        &body,
                        &format!("Setting fan to {speed}%"),
                    );
                }
            }
            "harlo" => {
                let body = if speed == "auto" {
                    "{\"mode\":\"auto\",\"sensor\":\"asic\",\"target_c\":55}".to_owned()
                } else {
                    format!("{{\"percent\":{speed}}}")
                };
                let notice = if speed == "auto" {
                    "Setting fans to auto".to_owned()
                } else {
                    format!("Setting fans to {speed}%")
                };
                self.send_put(&format!("http://{ip}/api/v1/fans/1"), &body, &notice);
                self.send_put(&format!("http://{ip}/api/v1/fans/2"), &body, &notice);
            }
            "canaan" => {
                let speed_n = if speed == "auto" {
                    -1
                } else {
                    speed.parse::<i32>().unwrap_or(40)
                };
                let notice = if speed == "auto" {
                    "Setting fan to auto".to_owned()
                } else {
                    format!("Setting fan to {speed}%")
                };
                self.send_tcp_raw(miner.host, &format!("ascset|0,fan-spd,{speed_n}"), &notice);
            }
            "bitmainZEC" => self.start_zec_fan(miner, speed),
            _ => self.notice = "Fan control is not available for this miner.".to_owned(),
        }
    }

    fn apply_work_mode(&mut self, miner: &Miner, id: &str) {
        let mode = match id {
            "mode-0" => 0,
            "mode-2" => 2,
            _ => 1,
        };
        if miner.family != "canaan" {
            self.notice = "Work mode is a Canaan command. Bitaxe uses the fan control and the frequency it reports.".to_owned();
            return;
        }
        let label = canaan_mode_label(miner, mode);
        let field = if canaan_uses_worklevel(miner) { "worklevel" } else { "workmode" };
        let command = format!("ascset|0,{field},set,{mode}");
        self.send_tcp_raw(miner.host, &command, &format!("Setting {label}"));
    }

    fn send_patch(&mut self, url: &str, body: &str, notice: &str) {
        self.notice = notice.to_owned();
        let _ = FetchRequest::patch(url)
            .headers("Content-Type: application/json")
            .body(body.as_bytes())
            .timeout(Duration::from_millis(2_500))
            .send(on_fetch);
    }

    fn send_post(&mut self, url: &str, notice: &str) {
        self.notice = notice.to_owned();
        let _ = FetchRequest::post(url)
            .timeout(Duration::from_millis(2_500))
            .send(on_fetch);
    }

    fn send_post_json(&mut self, url: &str, body: &str, notice: &str) {
        self.notice = notice.to_owned();
        let _ = FetchRequest::post(url)
            .headers("Content-Type: application/json")
            .body(body.as_bytes())
            .timeout(Duration::from_millis(2_500))
            .send(on_fetch);
    }

    fn send_put(&mut self, url: &str, body: &str, notice: &str) {
        self.notice = notice.to_owned();
        let _ = FetchRequest::put(url)
            .headers("Content-Type: application/json")
            .body(body.as_bytes())
            .timeout(Duration::from_millis(2_500))
            .send(on_fetch);
    }

    fn send_braiins(&mut self, miner: &Miner, path: &str, body: &str, notice: &str) {
        self.notice = notice.to_owned();
        if miner.token.is_empty() {
            self.notice = "Send the Braiins login from HashWatcher, then try again.".to_owned();
            return;
        }
        let headers = format!("Content-Type: application/json\nAuthorization: {}", miner.token);
        let _ = FetchRequest::put(&format!("http://{}{path}", miner.ip))
            .headers(&headers)
            .body(body.as_bytes())
            .timeout(Duration::from_millis(2_500))
            .send(on_fetch);
    }

    fn apply_braiins_tune(&mut self, miner: &Miner, id: &str) {
        let Some(index) = id.trim_start_matches("tune-").parse::<usize>().ok() else {
            return;
        };
        let Some(tune) = braiins_tunes(&miner.tune).into_iter().nth(index) else {
            self.notice = "That tune is not on this miner.".to_owned();
            return;
        };
        let (path, body, notice) = match tune.kind {
            BraiinsTuneKind::Hashrate => (
                "/api/v1/performance/hashrate-target",
                format!("{{\"terahash_per_second\":{}}}", tune.amount),
                format!("Setting hashrate target to {} TH/s", tune.amount),
            ),
            BraiinsTuneKind::Power => (
                "/api/v1/performance/power-target",
                format!("{{\"watt\":{}}}", tune.amount),
                format!("Setting power target to {} W", tune.amount),
            ),
        };
        self.send_braiins(miner, path, &body, &notice);
    }

    fn send_tcp_command(&mut self, host: u8, payload: &str, notice: &str) {
        self.send_tcp(host, payload, notice, true);
    }

    fn send_tcp_raw(&mut self, host: u8, payload: &str, notice: &str) {
        self.send_tcp(host, payload, notice, false);
    }

    fn send_tcp(&mut self, host: u8, payload: &str, notice: &str, terminate: bool) {
        self.notice = notice.to_owned();
        if self.sockets.len() >= MAX_SOCKETS {
            self.notice = "Port 4028 is busy. Try again in a moment.".to_owned();
            return;
        }
        let ip = self.ip(host);
        let Some(socket) = socket::tcp_connect(&ip, 4028, on_socket) else {
            self.notice = "Could not open port 4028.".to_owned();
            return;
        };
        let mut bytes = payload.as_bytes().to_vec();
        if terminate && !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        self.sockets.insert(
            socket.0.to_wire(),
            SocketJob {
                host,
                started: TICK.get(),
                connected: false,
                poll: false,
                control: true,
                payload: bytes,
                buf: Vec::new(),
                socket,
            },
        );
    }

    fn set_outbound(&mut self, on: bool) {
        self.outbound = on;
        if on {
            self.weather.error.clear();
            self.weather.ready = false;
            self.weather.fetched_at = 0;
        } else {
            self.weather.ready = false;
            self.weather.error = OUTBOUND_WEATHER_ERROR.to_owned();
            Self::cancel_fetch(&mut self.weather_fetch);
            Self::cancel_fetch(&mut self.suggest_fetch);
        }
        self.store_saver();
    }

    fn cancel_fetch(slot: &mut Option<u32>) {
        if let Some(raw) = slot.take() {
            if let Some(id) = FetchRequestId::from_wire(raw) {
                let _ = cancel(id);
            }
        }
    }

    fn maybe_check_update(&mut self, delta_ms: u32) {
        if !self.outbound || self.update_fetch.is_some() {
            return;
        }
        self.update_wait_ms = self.update_wait_ms.saturating_add(delta_ms.min(2_000));
        let due = if self.update_checked { UPDATE_EVERY_MS } else { UPDATE_FIRST_MS };
        if self.update_wait_ms < due {
            return;
        }
        self.update_wait_ms = 0;
        self.update_checked = true;
        let Some(id) = FetchRequest::get(RELEASE_URL)
            .headers("User-Agent: HashWatcher-Deck")
            .timeout(Duration::from_millis(8_000))
            .send(on_fetch)
        else {
            self.update_wait_ms = due.saturating_sub(15_000);
            return;
        };
        self.update_fetch = Some(id.to_wire());
    }

    fn finish_update_check(&mut self, response: &FetchResponse) {
        if !response.ok() {
            return;
        }
        let Some(version) = response.json().str("/version") else {
            return;
        };
        if !version_is_newer(&version, DECK_VERSION) {
            return;
        }
        let _ = FetchRequest::get("http://127.0.0.1:9418/cgi-bin/action?update")
            .timeout(Duration::from_millis(2_000))
            .send(on_fetch);
    }

    fn maybe_fetch_weather(&mut self) {
        if !self.outbound {
            self.weather.ready = false;
            self.weather.error = OUTBOUND_WEATHER_ERROR.to_owned();
            return;
        }
        if !self.saver_weather || self.place.trim().is_empty() || self.weather_fetch.is_some() {
            return;
        }
        if self.inflight.len() >= MAX_FETCHES {
            return;
        }
        let age = SystemTime::now().unix_secs.saturating_sub(self.weather.fetched_at);
        if self.weather.ready && age < 15 * 60 {
            return;
        }
        if !self.weather.ready && self.weather.fetched_at != 0 && age < 60 {
            return;
        }
        if self.weather.lat == 0.0 {
            self.weather_geo = true;
        }
        self.start_weather_fetch();
    }

    fn start_weather_fetch(&mut self) {
        if !self.outbound {
            self.weather.ready = false;
            self.weather.error = OUTBOUND_WEATHER_ERROR.to_owned();
            return;
        }
        let url = if self.weather_geo || self.weather.lat == 0.0 {
            self.weather_geo = true;
            format!(
                "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
                url_query(&self.place)
            )
        } else {
            format!(
                "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,apparent_temperature,is_day,weather_code,relative_humidity_2m,wind_speed_10m,wind_direction_10m&timezone=auto",
                self.weather.lat, self.weather.lon
            )
        };
        let Some(id) = FetchRequest::get(&url).timeout(Duration::from_millis(6_000)).send(on_fetch) else {
            return;
        };
        self.weather_fetch = Some(id.to_wire());
    }

    fn finish_weather(&mut self, response: &FetchResponse) {
        if !self.outbound {
            self.weather.ready = false;
            self.weather.error = OUTBOUND_WEATHER_ERROR.to_owned();
            return;
        }
        let text = response.text().unwrap_or("");
        if text.is_empty() {
            self.weather.error = "Weather is unavailable.".to_owned();
            self.weather.ready = false;
            self.weather.fetched_at = SystemTime::now().unix_secs;
            return;
        }
        let doc = response.json();
        if self.weather_geo {
            let Some(lat) = doc.f64("/results/0/latitude") else {
                self.weather.error = "No weather location matched.".to_owned();
                self.weather.ready = false;
                self.weather.fetched_at = SystemTime::now().unix_secs;
                return;
            };
            self.weather.lat = lat;
            self.weather.lon = doc.f64("/results/0/longitude").unwrap_or(0.0);
            let name = doc.str("/results/0/name").unwrap_or_default();
            let admin = doc.str("/results/0/admin1").unwrap_or_default();
            let country = doc.str("/results/0/country_code").unwrap_or_default();
            self.weather.label = [name, admin, country]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            if self.weather.label.is_empty() {
                self.weather.label = self.place.clone();
            }
            self.weather.error.clear();
            self.weather_geo = false;
            self.start_weather_fetch();
            return;
        }
        let Some(temp) = doc.f64("/current/temperature_2m") else {
            self.weather.error = "Weather did not return conditions.".to_owned();
            self.weather.ready = false;
            self.weather.fetched_at = SystemTime::now().unix_secs;
            return;
        };
        self.weather.temp_c = temp;
        self.weather.feels_c = doc.f64("/current/apparent_temperature").unwrap_or(temp);
        self.weather.wind_kmh = doc.f64("/current/wind_speed_10m").unwrap_or(0.0);
        self.weather.wind_deg = doc.f64("/current/wind_direction_10m");
        self.weather.humidity = doc.i64("/current/relative_humidity_2m").unwrap_or(0);
        self.weather.code = doc.i64("/current/weather_code").unwrap_or(0);
        self.weather.is_day = doc.i64("/current/is_day").unwrap_or(1) != 0;
        self.weather.ready = true;
        self.weather.error.clear();
        self.weather.fetched_at = SystemTime::now().unix_secs;
    }

    fn search_places(&mut self) {
        if !self.outbound {
            self.places.clear();
            self.suggest_fetch = None;
            return;
        }
        let query = self.place.trim();
        if query.chars().count() < 2 {
            self.places.clear();
            self.suggest_fetch = None;
            return;
        }
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=4&language=en&format=json",
            url_query(query)
        );
        let Some(id) = FetchRequest::get(&url).timeout(Duration::from_millis(4_000)).send(on_fetch) else {
            return;
        };
        self.suggest_fetch = Some(id.to_wire());
    }

    fn finish_suggestions(&mut self, response: &FetchResponse) {
        let doc = response.json();
        let mut hits = Vec::new();
        for index in 0..4 {
            let name = doc.str(&format!("/results/{index}/name")).unwrap_or_default();
            let Some(lat) = doc.f64(&format!("/results/{index}/latitude")) else {
                break;
            };
            if name.is_empty() {
                break;
            }
            let admin = doc.str(&format!("/results/{index}/admin1")).unwrap_or_default();
            let country = doc.str(&format!("/results/{index}/country_code")).unwrap_or_default();
            let label = [name.clone(), admin, country]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            hits.push(PlaceHit {
                name,
                label,
                lat,
                lon: doc.f64(&format!("/results/{index}/longitude")).unwrap_or(0.0),
            });
        }
        self.places = hits;
    }

    fn choose_place(&mut self, hit: PlaceHit) {
        self.place = hit.name;
        self.place_edit = false;
        self.places.clear();
        self.suggest_fetch = None;
        self.weather = Weather::default();
        self.weather.lat = hit.lat;
        self.weather.lon = hit.lon;
        self.weather.label = hit.label;
        self.weather_geo = false;
        self.weather_fetch = None;
        self.place_local = true;
        self.store_saver();
    }
}

fn on_fetch(response: &FetchResponse) {
    APP.with(|app| app.borrow_mut().handle_http(response));
    let paused = APP.with(|app| app.borrow().settings);
    if !paused {
        APP.with(|app| app.borrow_mut().fill());
        schedule_poll();
    }
    request_frame();
}

fn on_socket(socket: Socket, event: &SocketEvent<'_>) {
    APP.with(|app| app.borrow_mut().handle_socket(socket, event));
    let paused = APP.with(|app| app.borrow().settings);
    if !paused {
        APP.with(|app| app.borrow_mut().fill());
    }
    request_frame();
}

fn schedule_poll() {
    let wait = APP.with(|app| {
        let app = app.borrow();
        if app.phase == Phase::Live && app.queue.is_empty() && app.inflight.is_empty() && app.sockets.is_empty() {
            Some(app.poll_ms)
        } else {
            None
        }
    });
    if let Some(wait) = wait {
        request_frame_after(wait);
    }
}

fn classify(step: u8, ip: &str, host: u8, text: &str, doc: &JsonDoc) -> Option<Miner> {
    let blob = text.to_ascii_lowercase();
    let mut miner = blank(host, ip);
    match step {
        0 if doc.f64("/hashRate").is_some() || doc.i64("/hashRate").is_some() => {
            miner.family = "bitaxe";
            miner.name = doc.str("/hostname").unwrap_or_default();
            miner.model = doc
                .str("/ASICModel")
                .or_else(|| doc.str("/deviceModel"))
                .unwrap_or_default();
            apply_bitaxe(&mut miner, doc);
        }
        1 if blob.contains("asic_temp_c")
            || blob.contains("hashrate_ghs")
            || blob.contains("harlo") =>
        {
            miner.family = "harlo";
            apply_harlo(&mut miner, doc);
        }
        2 if blob.contains("bos") || blob.contains("braiins") || blob.contains("bosminer") => {
            miner.family = "braiins";
            miner.model = "Braiins OS".to_owned();
        }
        3 if blob.contains("vnish") || blob.contains("hr_measure") || blob.contains("hr_realtime") => {
            miner.family = "vnish";
            miner.name = doc.str("/hostname").unwrap_or_default();
            miner.model = doc.str("/miner").or_else(|| doc.str("/model")).unwrap_or_default();
        }
        4 if blob.contains("hashrate_ghs") || blob.contains("model_name") => {
            miner.family = "luckyMiner";
            miner.name = doc.str("/minername").unwrap_or_default();
            miner.model = doc.str("/model_name").unwrap_or_default();
            miner.hashrate_ths = ghs_to_ths(num(doc, "/hashrate_ghs"));
            miner.power_w = num(doc, "/power_watts");
        }
        5 if blob.contains("zyber") || blob.contains("\"zb") => {
            miner.family = "zyberos";
            miner.name = doc.str("/hostname").unwrap_or_default();
            miner.model = doc.str("/boardVersion").unwrap_or_default();
        }
        6 if blob.contains("futurebit") || blob.contains("apollo") => {
            miner.family = "futurebit";
            miner.model = "FutureBit".to_owned();
        }
        _ => return None,
    }
    miner.reachable = true;
    remember(&mut miner);
    Some(miner)
}

fn classify_cgminer(ip: &str, host: u8, text: &str, doc: &JsonDoc) -> Miner {
    let blob = text.to_ascii_lowercase();
    let mut miner = blank(host, ip);
    miner.reachable = true;
    miner.family = if blob.contains("luxminer") || blob.contains("luxos") {
        "luxos"
    } else if blob.contains("avalon") || blob.contains("canaan") {
        "canaan"
    } else if blob.contains("equihash") || blob.contains("\"zec\"") {
        "bitmainZEC"
    } else {
        "cgminer"
    };
    miner.model = doc
        .str("/VERSION/0/Type")
        .or_else(|| doc.str("/VERSION/0/Description"))
        .or_else(|| doc.str("/VERSION/0/Miner"))
        .unwrap_or_default();
    if looks_like_version(&miner.model) {
        miner.model.clear();
    }
    miner.name.clone_from(&miner.model);
    miner
}

fn hue_rgb(hue: u16) -> (u8, u8, u8) {
    let hue = f32::from(hue % 360);
    let sector = hue / 60.0;
    let index = sector as u32;
    let fraction = sector - index as f32;
    let rising = (fraction * 255.0) as u8;
    let falling = ((1.0 - fraction) * 255.0) as u8;
    match index {
        0 => (255, rising, 0),
        1 => (falling, 255, 0),
        2 => (0, 255, rising),
        3 => (0, falling, 255),
        4 => (rising, 0, 255),
        _ => (255, 0, falling),
    }
}

fn note_miss(miner: &mut Miner) {
    miner.misses = miner.misses.saturating_add(1);
    if miner.misses >= 3 {
        miner.reachable = false;
    }
}

fn remember(miner: &mut Miner) {
    miner.reachable = true;
    miner.misses = 0;
    if let Some(rate) = miner.hashrate_ths {
        record_chart(miner, SystemTime::now().unix_secs, rate);
    }
}

fn record_chart(miner: &mut Miner, now: i64, value: f64) {
    let hash = value as f32;
    if !hash.is_finite() {
        return;
    }
    if let Some(last) = miner.chart.last_mut() {
        if now <= last.at {
            last.at = now;
            last.hash = hash;
            miner.chart_dirty = true;
            return;
        }
    }
    miner.chart.push(ChartSample { at: now, hash });
    let cutoff = now - 24 * 60 * 60;
    if miner.chart.first().is_some_and(|sample| sample.at < cutoff) {
        miner.chart.retain(|sample| sample.at >= cutoff);
    }
    thin_chart(&mut miner.chart, now);
    miner.chart_dirty = true;
}

/// Keep every poll for 5 minutes, then thin older samples so 24 hours stays small.
fn thin_chart(samples: &mut Vec<ChartSample>, now: i64) {
    if samples.len() < 3 {
        return;
    }
    let mut kept = Vec::with_capacity(samples.len());
    let mut last_at = i64::MIN;
    let end = samples.len() - 1;
    for (index, sample) in samples.iter().enumerate() {
        let age = now.saturating_sub(sample.at);
        let gap = if age <= 5 * 60 {
            0
        } else if age <= 30 * 60 {
            15
        } else if age <= 6 * 60 * 60 {
            60
        } else {
            300
        };
        if index == end || sample.at.saturating_sub(last_at) >= gap {
            kept.push(*sample);
            last_at = sample.at;
        }
    }
    *samples = kept;
}

fn load_chart(ip: &str) -> Vec<ChartSample> {
    let Some(text) = bmc_wasm_sdk::kv::get_string(&format!("chart-{ip}")) else {
        return Vec::new();
    };
    let now = SystemTime::now().unix_secs;
    let mut samples = Vec::new();
    for part in text.split(';') {
        let Some((at, hash)) = part.split_once(',') else {
            continue;
        };
        let Ok(at) = at.parse::<i64>() else {
            continue;
        };
        let Ok(hash) = hash.parse::<f32>() else {
            continue;
        };
        if hash.is_finite() && at >= now - 24 * 60 * 60 && at <= now + 120 {
            samples.push(ChartSample { at, hash });
        }
    }
    thin_chart(&mut samples, now);
    samples
}

fn save_chart(ip: &str, samples: &[ChartSample]) {
    let mut body = String::new();
    for sample in samples {
        if !body.is_empty() {
            body.push(';');
        }
        body.push_str(&format!("{},{:.3}", sample.at, sample.hash));
    }
    bmc_wasm_sdk::kv::set(&format!("chart-{ip}"), body.as_bytes());
}

fn apply_http_poll(miner: &mut Miner, text: &str, doc: &JsonDoc) {
    let saved_hash = miner.hashrate_ths;
    let saved_power = miner.power_w;
    let saved_temp = miner.temp_c;
    let saved_shares = miner.shares_accepted;
    let saved_uptime = miner.uptime_s;
    match miner.family {
        "bitaxe" => apply_bitaxe(miner, doc),
        "harlo" => apply_harlo(miner, doc),
        "luckyMiner" => apply_lucky_overview(miner, doc),
        "braiins" => apply_braiins_stats(miner, doc),
        "vnish" => {
            let rate = first_num(doc, &["/miner/instant_hashrate", "/miner/hr_realtime", "/instant_hashrate"]);
            let unit = doc.str("/hr_measure").unwrap_or_default().to_ascii_lowercase();
            miner.hashrate_ths = rate.map(|value| match unit.as_str() {
                "gh/s" | "ghs" | "gh" => value / 1000.0,
                "mh/s" | "mhs" => value / 1_000_000.0,
                _ => value,
            });
            miner.power_w = first_num(doc, &["/miner/power_consumption", "/miner/power_usage"]);
            miner.temp_c = first_num(doc, &["/miner/chip_temp", "/miner/pcb_temp"]);
        }
        "zyberos" => apply_zyber(miner, doc),
        "futurebit" => {
            let _ = text;
        }
        _ => {}
    }
    if miner.hashrate_ths.is_none() {
        miner.hashrate_ths = scrape_http_hashrate(miner.family, text);
    }
    if has_reading(miner) {
        remember(miner);
    } else if miner.family == "braiins" && saved_hash.is_some() {
        miner.hashrate_ths = saved_hash;
        miner.power_w = saved_power;
        miner.temp_c = saved_temp;
        miner.shares_accepted = saved_shares;
        miner.uptime_s = saved_uptime;
    } else {
        miner.hashrate_ths = saved_hash;
        miner.power_w = saved_power;
        miner.temp_c = saved_temp;
        miner.shares_accepted = saved_shares;
        miner.uptime_s = saved_uptime;
        note_miss(miner);
    }
}

fn commit_best_diff(miner: &mut Miner, next: Option<f64>) {
    let Some(next) = next.filter(|value| value.is_finite() && *value > 0.0) else {
        return;
    };
    let previous = miner.best_diff;
    if previous.is_some_and(|value| next <= value * 1.001) {
        return;
    }
    miner.best_diff = Some(next);
    if previous.is_none() {
        return;
    }
    let name = if miner.name.trim().is_empty() {
        miner.ip.clone()
    } else {
        miner.name.clone()
    };
    let graphic = BEST_GRAPHIC.with(|cell| {
        let next_graphic = cell.get() ^ 1;
        cell.set(next_graphic);
        next_graphic
    });
    BEST_QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= 3 {
            queue.pop_front();
        }
        queue.push_back(BestPopup {
            name,
            pool: miner.pool.clone(),
            difficulty: next,
            graphic,
            kind: 0,
        });
    });
}

fn apply_harlo(miner: &mut Miner, doc: &JsonDoc) {
    miner.hashrate_ths = ghs_to_ths(first_positive(
        doc,
        &[
            "/accepted_pool_hashrate_ghs",
            "/pool_hashrate_ghs",
            "/average_hashrate_ghs",
            "/local_hashrate_ghs",
            "/hashrate_ghs",
        ],
    ));
    miner.power_w = first_positive(doc, &["/wall_power_w", "/rail_power_w", "/power_watts", "/power"]);
    miner.temp_c = first_positive(doc, &["/asic_temp_c", "/chip_temp", "/max_chip_temp", "/temperature", "/temp"]);
    miner.vr_temp = first_positive(doc, &["/vr_temp_c", "/vrTemp"]);
    miner.fan = first_positive(doc, &["/fan_percent", "/fanspeed"]);
    miner.fan_rpm = None;
    set_fan_slots(miner, &[(miner.fan.unwrap_or(0.0), 0.0)]);
    miner.uptime_s = whole(doc, "/uptime_seconds").or_else(|| whole(doc, "/uptimeSeconds"));
    miner.shares_accepted = whole(doc, "/accepted_shares").or_else(|| whole(doc, "/sharesAccepted"));
    miner.shares_rejected = whole(doc, "/rejected_shares").or_else(|| whole(doc, "/sharesRejected"));
    commit_best_diff(miner, first_positive(doc, &["/best_share", "/bestDiff"]));
    miner.best_session = first_positive(doc, &["/session_best_share", "/bestSessionDiff"]);
    miner.frequency = first_positive(doc, &["/frequency_mhz", "/current_frequency_mhz", "/frequency"]);
    miner.voltage = first_positive(doc, &["/core_mv", "/measured_core_mv", "/voltage"]);
    if miner.name.is_empty() {
        miner.name = doc
            .str("/hostname")
            .or_else(|| doc.str("/miner_name"))
            .unwrap_or_default();
    }
    if miner.model.is_empty() {
        miner.model = doc
            .str("/profile")
            .or_else(|| doc.str("/model"))
            .unwrap_or_default();
    }
}

fn apply_braiins_stats(miner: &mut Miner, doc: &JsonDoc) {
    let ghs = first_positive(
        doc,
        &[
            "/miner_stats/real_hashrate/last_5s/gigahash_per_second",
            "/miner_stats/real_hashrate/last_15s/gigahash_per_second",
            "/miner_stats/real_hashrate/last_1m/gigahash_per_second",
            "/miner_stats/real_hashrate/last_5m/gigahash_per_second",
            "/miner_stats/nominal_hashrate/gigahash_per_second",
        ],
    );
    if let Some(ghs) = ghs {
        miner.hashrate_ths = Some(ghs / 1000.0);
    }
    if let Some(watts) = first_positive(
        doc,
        &[
            "/power_stats/approximated_consumption/watt",
            "/power_stats/consumption/watt",
        ],
    ) {
        miner.power_w = Some(watts);
    }
    if let Some(shares) = whole(doc, "/pool_stats/accepted_shares") {
        if !miner.share_locked {
            miner.shares_seen = Some(shares);
            miner.share_locked = true;
        }
        miner.shares_accepted = Some(shares);
    }
    if let Some(rejected) = whole(doc, "/pool_stats/rejected_shares") {
        miner.shares_rejected = Some(rejected);
    }
    commit_best_diff(miner, first_positive(doc, &["/pool_stats/best_share", "/miner_stats/best_share"]));
    if let (Some(error_mh), Some(real_gh)) = (
        first_num(doc, &["/miner_stats/error_hashrate/megahash_per_second"]),
        ghs,
    ) {
        if real_gh > 0.0 {
            miner.asic_pct = Some(error_mh / (real_gh * 10.0));
        }
    }
}

fn apply_braiins_cooling(miner: &mut Miner, doc: &JsonDoc) {
    if let Some(temp) = first_positive(doc, &["/highest_temperature/temperature/degree_c"]) {
        miner.temp_c = Some(temp);
    }
    let mut slots = Vec::new();
    for index in 0..8 {
        let rpm = num(doc, &format!("/fans/{index}/rpm")).unwrap_or(0.0);
        let ratio = num(doc, &format!("/fans/{index}/target_speed_ratio")).unwrap_or(0.0);
        if rpm > 0.0 || ratio > 0.0 {
            let pct = if ratio > 0.0 { (ratio * 100.0).clamp(1.0, 100.0) } else { 0.0 };
            slots.push((pct, rpm));
        }
    }
    if !slots.is_empty() {
        set_fan_slots(miner, &slots);
    }
}

fn apply_braiins_boards(miner: &mut Miner, doc: &JsonDoc) {
    let mut hottest = 0.0_f64;
    let mut mhz = 0.0_f64;
    for index in 0..4 {
        if let Some(temp) = first_positive(
            doc,
            &[
                &format!("/hashboards/{index}/highest_chip_temp/temperature/degree_c"),
                &format!("/hashboards/{index}/board_temp/degree_c"),
            ],
        ) {
            hottest = hottest.max(temp);
        }
        if let Some(hertz) = first_positive(doc, &[&format!("/hashboards/{index}/current_frequency/hertz")]) {
            mhz = mhz.max(hertz / 1_000_000.0);
        }
    }
    if hottest > 0.0 {
        miner.temp_c = Some(hottest);
    }
    if mhz > 0.0 {
        miner.frequency = Some(mhz);
    }
}

fn apply_pool_list(miner: &mut Miner, doc: &JsonDoc) {
    let mut chosen: Option<(i64, String, String)> = None;
    for index in 0..8 {
        let url = doc
            .str(&format!("/POOLS/{index}/URL"))
            .or_else(|| doc.str(&format!("/POOLS/{index}/Stratum URL")))
            .unwrap_or_default();
        if url.is_empty() {
            continue;
        }
        let user = doc.str(&format!("/POOLS/{index}/User")).unwrap_or_default();
        let priority = doc.i64(&format!("/POOLS/{index}/Priority")).unwrap_or(index as i64);
        let alive = doc
            .str(&format!("/POOLS/{index}/Status"))
            .unwrap_or_default()
            .eq_ignore_ascii_case("alive");
        let rank = if alive { priority } else { priority + 100 };
        if chosen.as_ref().is_none_or(|current| rank < current.0) {
            chosen = Some((rank, url, user));
        }
    }
    if let Some((_, url, user)) = chosen {
        miner.pool = url;
        if !user.is_empty() {
            miner.pool_user = user;
        }
        remember(miner);
    }
}

fn apply_board_temps(miner: &mut Miner, doc: &JsonDoc) {
    let mut hottest = 0.0_f64;
    for index in 0..8 {
        if let Some(chip) = num(doc, &format!("/TEMPS/{index}/Chip")).filter(|value| *value > 0.0) {
            hottest = hottest.max(chip);
        }
    }
    if hottest > 0.0 {
        miner.temp_c = Some(hottest);
        remember(miner);
    }
}

fn set_fan_slots(miner: &mut Miner, slots: &[(f64, f64)]) {
    miner.fan_pcts = [0.0; 8];
    miner.fan_rpms = [0.0; 8];
    let mut best_pct = 0.0_f64;
    let mut best_rpm = 0.0_f64;
    for (index, (pct, rpm)) in slots.iter().take(8).enumerate() {
        let pct = if *pct > 0.0 && *pct <= 100.0 { *pct } else { 0.0 };
        let rpm = if *rpm > 0.0 { *rpm } else { 0.0 };
        if pct == 0.0 && rpm == 0.0 {
            continue;
        }
        miner.fan_pcts[index] = pct;
        miner.fan_rpms[index] = rpm;
        if rpm > best_rpm {
            best_rpm = rpm;
            if pct > 0.0 {
                best_pct = pct;
            }
        } else if best_pct == 0.0 {
            best_pct = pct;
        }
    }
    if best_pct > 0.0 {
        miner.fan = Some(best_pct);
    }
    if best_rpm > 0.0 {
        miner.fan_rpm = Some(best_rpm);
    }
}

fn apply_board_fans(miner: &mut Miner, doc: &JsonDoc) {
    let mut slots = Vec::new();
    for index in 0..8 {
        let rpm = num(doc, &format!("/FANS/{index}/RPM")).unwrap_or(0.0);
        let speed = num(doc, &format!("/FANS/{index}/Speed")).unwrap_or(0.0);
        if rpm > 0.0 || speed > 0.0 {
            slots.push((speed, rpm));
        }
    }
    if slots.is_empty() {
        return;
    }
    set_fan_slots(miner, &slots);
    remember(miner);
}

fn apply_pool(miner: &mut Miner, doc: &JsonDoc) {
    let host = doc
        .str("/pools/0/host")
        .or_else(|| doc.str("/host"))
        .unwrap_or_default();
    if host.is_empty() {
        return;
    }
    miner.pool = match whole(doc, "/pools/0/port").or_else(|| whole(doc, "/port")) {
        Some(port) => format!("{host}:{port}"),
        None => host,
    };
    if let Some(user) = doc.str("/pools/0/user").or_else(|| doc.str("/user")) {
        if !user.is_empty() {
            miner.pool_user = user;
        }
    }
}

fn apply_bitaxe(miner: &mut Miner, doc: &JsonDoc) {
    miner.hashrate_ths = first_positive(doc, &["/hashRate", "/hashrate"]).map(|value| value / 1000.0);
    miner.power_w = first_positive(doc, &["/power", "/power_w"]);
    miner.temp_c = first_positive(doc, &["/temp", "/asic_temp_c"]);
    miner.vr_temp = first_positive(doc, &["/vrTemp", "/vr_temp_c"]);
    miner.uptime_s = whole(doc, "/uptimeSeconds").or_else(|| whole(doc, "/uptime_seconds"));
    if let Some(pool) = doc.str("/stratumURL").or_else(|| doc.str("/stratum_url")) {
        if !pool.is_empty() {
            miner.pool = pool;
        }
    }
    if let Some(user) = doc.str("/stratumUser").or_else(|| doc.str("/stratum_user")) {
        if !user.is_empty() {
            miner.pool_user = user;
        }
    }
    miner.shares_accepted = whole(doc, "/sharesAccepted").or_else(|| whole(doc, "/accepted_shares"));
    miner.shares_rejected = whole(doc, "/sharesRejected").or_else(|| whole(doc, "/rejected_shares"));
    miner.best_session = first_positive(doc, &["/bestSessionDiff", "/session_best_share"]);
    commit_best_diff(miner, first_positive(doc, &["/bestDiff", "/best_share"]));
    if miner.best_diff.is_none() {
        miner.best_diff = miner.best_session;
    }
    miner.frequency = first_positive(doc, &["/frequency", "/frequency_mhz"]);
    miner.default_frequency = num(doc, "/defaultFrequency");
    miner.eco_frequency = num(doc, "/ecoFrequency");
    miner.voltage = first_positive(doc, &["/coreVoltage", "/core_mv", "/voltage"]);
    let mut slots = Vec::new();
    for index in 0..8 {
        let pct = num(doc, &format!("/fans/{index}/speedPerc"))
            .or_else(|| num(doc, &format!("/fans/{index}/speed")))
            .unwrap_or(0.0);
        let rpm = num(doc, &format!("/fans/{index}/rpm")).unwrap_or(0.0);
        if pct > 0.0 || rpm > 0.0 {
            slots.push((pct, rpm));
        }
    }
    if slots.is_empty() {
        let pct = first_positive(doc, &["/fanspeed", "/fan_percent"]).unwrap_or(0.0);
        let rpm = first_positive(doc, &["/fanrpm"]).unwrap_or(0.0);
        if pct > 0.0 || rpm > 0.0 {
            slots.push((pct, rpm));
        }
    }
    if !slots.is_empty() {
        set_fan_slots(miner, &slots);
    }
    if miner.hashrate_ths.is_none() {
        miner.hashrate_ths = first_positive(doc, &["/miner/hashRate"]).map(|value| value / 1000.0);
    }
    if miner.temp_c.is_none() {
        miner.temp_c = first_positive(doc, &["/temps/asic"]);
    }
    if miner.vr_temp.is_none() {
        miner.vr_temp = first_positive(doc, &["/temps/vcore"]);
    }
    if miner.power_w.is_none() {
        miner.power_w = first_positive(doc, &["/power/power"]);
    }
    if miner.shares_accepted.is_none() {
        miner.shares_accepted = whole(doc, "/miner/sAccepted");
    }
    if miner.uptime_s.is_none() {
        miner.uptime_s = whole(doc, "/miner/uptimeSeconds");
    }
    if miner.frequency.is_none() {
        miner.frequency = first_positive(doc, &["/asic/freqReq"]);
    }
    if miner.voltage.is_none() {
        miner.voltage = first_positive(doc, &["/asic/vcoreReal"]);
    }
    if miner.name.is_empty() {
        miner.name = doc
            .str("/identity/displayName")
            .or_else(|| doc.str("/identity/hostName"))
            .unwrap_or_default();
    }
    if miner.name.is_empty() {
        miner.name = doc.str("/hostname").unwrap_or_default();
    }
    if miner.model.is_empty() {
        miner.model = doc
            .str("/ASICModel")
            .or_else(|| doc.str("/deviceModel"))
            .unwrap_or_default();
    }
}

fn apply_zyber(miner: &mut Miner, doc: &JsonDoc) {
    if let Some(rate) = first_positive(doc, &["/hashRate", "/hashRate_1m", "/hashRate_10m", "/hashrate"]) {
        miner.hashrate_ths = Some(rate / 1000.0);
    }
    if let Some(power) = first_positive(doc, &["/power", "/powerWatts"]) {
        miner.power_w = Some(power);
    }
    if let Some(temp) = first_positive(doc, &["/temp", "/chipTemp", "/temp2"]) {
        miner.temp_c = Some(temp);
    }
    if let Some(vr) = first_positive(doc, &["/vrTemp"]) {
        miner.vr_temp = Some(vr);
    }
    if let Some(fan) = first_positive(doc, &["/fanspeed", "/fan_percent"]) {
        set_fan_slots(miner, &[(fan, first_positive(doc, &["/fanrpm"]).unwrap_or(0.0))]);
    }
    if let Some(shares) = whole(doc, "/sharesAccepted") {
        miner.shares_accepted = Some(shares);
    }
    if let Some(uptime) = whole(doc, "/uptimeSeconds") {
        miner.uptime_s = Some(uptime);
    }
    commit_best_diff(miner, first_positive(doc, &["/bestDiff"]));
    if let Some(session) = first_positive(doc, &["/bestSessionDiff"]) {
        miner.best_session = Some(session);
    }
    if let Some(mhz) = first_positive(doc, &["/frequency", "/actualFrequency"]) {
        miner.frequency = Some(mhz);
    }
    if miner.pool.is_empty() {
        miner.pool = doc.str("/stratumURL").unwrap_or_default();
    }
    if miner.pool_user.is_empty() {
        miner.pool_user = doc.str("/stratumUser").unwrap_or_default();
    }
}

fn apply_lucky_overview(miner: &mut Miner, doc: &JsonDoc) {
    if let Some(ghs) = first_positive(
        doc,
        &["/hashrate_ghs", "/local_hashrate_ghs", "/pool_hashrate_ghs", "/hashRate"],
    ) {
        miner.hashrate_ths = Some(ghs / 1000.0);
    }
    if let Some(power) = first_positive(doc, &["/power_watts", "/power"]) {
        miner.power_w = Some(power);
    }
    if let Some(temp) = first_positive(
        doc,
        &["/chipTemp", "/temp_core_1", "/chip_temp", "/asic_temp_c", "/temp", "/temperature"],
    ) {
        miner.temp_c = Some(temp);
    }
    if let Some(vr) = first_positive(doc, &["/temp_vr", "/vrTemp", "/vr_temp"]) {
        miner.vr_temp = Some(vr);
    }
    let percent = first_positive(doc, &["/fan_speed_pct", "/fan_percent", "/fanspeed"]).unwrap_or(0.0);
    let mut slots = Vec::new();
    for index in 1..=4 {
        if let Some(rpm) = num(doc, &format!("/fan{index}_rpm")).filter(|value| *value > 0.0) {
            slots.push((percent, rpm));
        }
    }
    if slots.is_empty() {
        let rpm = first_positive(doc, &["/fanrpm"]).unwrap_or(0.0);
        if percent > 0.0 || rpm > 0.0 {
            slots.push((percent, rpm));
        }
    }
    if !slots.is_empty() {
        set_fan_slots(miner, &slots);
    }
    if let Some(shares) = whole(doc, "/shares_accepted").or_else(|| whole(doc, "/sharesAccepted")) {
        miner.shares_accepted = Some(shares);
    }
    if let Some(rejected) = whole(doc, "/shares_rejected").or_else(|| whole(doc, "/sharesRejected")) {
        miner.shares_rejected = Some(rejected);
    }
    if let Some(uptime) = whole(doc, "/uptime_sec")
        .or_else(|| whole(doc, "/uptime_seconds"))
        .or_else(|| whole(doc, "/uptimeSeconds"))
    {
        miner.uptime_s = Some(uptime);
    }
    commit_best_diff(miner, first_positive(doc, &["/best_diff", "/bestDiff"]));
    if let Some(session) = first_positive(doc, &["/best_diff_session", "/bestSessionDiff"]) {
        miner.best_session = Some(session);
    }
    if let Some(mhz) = first_positive(doc, &["/freq_actual_mhz", "/frequency"]) {
        miner.frequency = Some(mhz);
    }
    if let Some(mv) = first_positive(doc, &["/voltage_actual_mv", "/coreVoltage"]) {
        miner.voltage = Some(mv);
    }
    if let Some(url) = doc.str("/stratum_url").or_else(|| doc.str("/stratumURL")) {
        if !url.is_empty() {
            let port = whole(doc, "/stratumPort").or_else(|| whole(doc, "/stratum_port"));
            miner.pool = match port {
                Some(port) if !url.contains(':') => format!("{url}:{port}"),
                _ => url,
            };
        }
    }
    if let Some(user) = doc.str("/stratum_user").or_else(|| doc.str("/stratumUser")) {
        if !user.is_empty() {
            miner.pool_user = user;
        }
    }
    if let Some(error) = num(doc, "/errorPercentage") {
        miner.asic_pct = Some(error);
    }
    if let Some(best) = labeled_difficulty(doc, &["/best_diff", "/bestDiff"]) {
        commit_best_diff(miner, Some(best));
    }
    if let Some(session) = labeled_difficulty(doc, &["/best_diff_session", "/bestSessionDiff"]) {
        miner.best_session = Some(session);
    }
}

fn frame_ready(buf: &[u8]) -> bool {
    if buf.contains(&0) {
        return true;
    }
    let mut depth = 0_i32;
    let mut started = false;
    let mut in_string = false;
    let mut escape = false;
    for &byte in buf {
        if in_string {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                started = true;
            }
            b'}' | b']' => {
                depth -= 1;
                if started && depth <= 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn bracket_value(text: &str, key: &str) -> Option<f64> {
    let needle = format!("{key}[");
    let rest = text.split(&needle).nth(1)?;
    let inside = rest.split(']').next()?.trim().trim_end_matches('%');
    let token = inside.split_whitespace().next()?;
    token.parse::<f64>().ok().filter(|value| value.is_finite() && *value > 0.0)
}

fn ps_watts(text: &str) -> Option<f64> {
    let inside = text.split("PS[").nth(1)?.split(']').next()?;
    let numbers: Vec<f64> = inside
        .split_whitespace()
        .filter_map(|token| token.parse::<f64>().ok())
        .collect();
    numbers.get(6).copied().filter(|watts| *watts > 0.0)
}

fn apply_aux_poll(miner: &mut Miner, text: &str) {
    match miner.family {
        "canaan" => {
            if let Some(temp) = bracket_value(text, "TAvg") {
                miner.temp_c = Some(temp);
            }
            let percent = bracket_value(text, "FanR").unwrap_or(0.0);
            let mut slots = Vec::new();
            for name in ["Fan1", "Fan2", "Fan3", "Fan4"] {
                if let Some(rpm) = bracket_value(text, name) {
                    slots.push((percent, rpm));
                }
            }
            if slots.is_empty() && percent > 0.0 {
                slots.push((percent, 0.0));
            }
            if !slots.is_empty() {
                set_fan_slots(miner, &slots);
            }
            if let Some(ghs) = bracket_value(text, "GHSspd") {
                miner.hashrate_ths = Some(ghs / 1000.0);
            }
            if let Some(watts) = ps_watts(text) {
                miner.power_w = Some(watts);
            }
            if let Some(mhz) = bracket_value(text, "Freq") {
                miner.frequency = Some(mhz);
            }
            remember(miner);
        }
        "bitmainZEC" => {
            let temps: Vec<f64> = ["temp2_1", "temp2_2", "temp2_3", "temp2_4"]
                .into_iter()
                .filter_map(|key| scrape_number(text, key).filter(|value| *value > 0.0))
                .collect();
            if !temps.is_empty() {
                miner.temp_c = Some(temps.iter().sum::<f64>() / temps.len() as f64);
            }
            let boards: Vec<f64> = ["temp1", "temp2", "temp3", "temp4"]
                .into_iter()
                .filter_map(|key| scrape_number(text, key).filter(|value| *value > 0.0))
                .collect();
            if !boards.is_empty() {
                miner.board_temp = Some(boards.iter().sum::<f64>() / boards.len() as f64);
            }
            miner.vr_temp = None;
            miner.best_diff = None;
            miner.best_session = None;
            miner.power_w = None;
            let mut rpms = Vec::new();
            for key in ["fan1", "fan2", "fan3", "fan4", "fan5", "fan6"] {
                if let Some(rpm) = scrape_number(text, key).filter(|value| *value > 0.0) {
                    rpms.push(rpm);
                }
            }
            let peak = rpms.iter().copied().fold(0.0_f64, f64::max);
            let slots: Vec<(f64, f64)> = rpms
                .iter()
                .map(|rpm| (zec_fan_percent(*rpm, peak), *rpm))
                .collect();
            if !slots.is_empty() {
                set_fan_slots(miner, &slots);
            }
            if let Some(ghs) = scrape_number(text, "GHS 5s").or_else(|| scrape_number(text, "GHS av")) {
                let ksol = if ghs >= 1_000.0 { ghs / 1_000.0 } else { ghs };
                miner.hashrate_ths = Some(ksol);
            }
            remember(miner);
        }
        _ => {}
    }
}

fn scrape_number(text: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{key}\"");
    for rest in text.split(&needle).skip(1) {
        let rest = rest.trim_start();
        if rest.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphanumeric()) {
            continue;
        }
        let rest = rest.trim_start_matches(':').trim_start().trim_start_matches('"');
        let token: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E'))
            .collect();
        if let Ok(value) = token.parse::<f64>() {
            if value.is_finite() && value > 0.0 {
                return Some(value);
            }
        }
    }
    None
}

fn scrape_http_hashrate(family: &str, text: &str) -> Option<f64> {
    let ghs = scrape_number(text, "hashRate")
        .or_else(|| scrape_number(text, "hashrate"))
        .or_else(|| scrape_number(text, "hashrate_ghs"))
        .or_else(|| scrape_number(text, "local_hashrate_ghs"));
    ghs.map(|value| if family == "bitmainZEC" { value } else { value / 1000.0 })
}

fn apply_cgminer_poll(miner: &mut Miner, text: &str, doc: &JsonDoc) {
    let saved_hash = miner.hashrate_ths;
    let saved_power = miner.power_w;
    let saved_temp = miner.temp_c;
    let saved_shares = miner.shares_accepted;
    let saved_uptime = miner.uptime_s;
    let ths = first_num(doc, &["/SUMMARY/0/THS av", "/SUMMARY/0/THS 5s"])
        .or_else(|| scrape_number(text, "THS av"))
        .or_else(|| scrape_number(text, "THS 5s"));
    let ghs = first_num(
        doc,
        &[
            "/SUMMARY/0/GHS 5s",
            "/SUMMARY/0/GHS av",
            "/SUMMARY/0/GHS 1m",
            "/SUMMARY/0/GHS 15m",
        ],
    )
    .or_else(|| scrape_number(text, "GHS 5s"))
    .or_else(|| scrape_number(text, "GHS av"))
    .or_else(|| scrape_number(text, "GHSspd"));
    let mhs = first_num(
        doc,
        &[
            "/SUMMARY/0/MHS 5s",
            "/SUMMARY/0/MHS 5m",
            "/SUMMARY/0/MHS av",
            "/SUMMARY/0/MHS 1m",
            "/SUMMARY/0/MHS 15m",
        ],
    )
    .or_else(|| scrape_number(text, "MHS 5s"))
    .or_else(|| scrape_number(text, "MHS 5m"))
    .or_else(|| scrape_number(text, "MHS av"))
    .or_else(|| scrape_number(text, "MHS 15m"));
    if let Some(rate) = ths
        .or_else(|| {
            ghs.map(|value| {
                if miner.family == "bitmainZEC" {
                    if value >= 1_000.0 { value / 1_000.0 } else { value }
                } else {
                    value / 1_000.0
                }
            })
        })
        .or_else(|| mhs.map(|value| value / 1_000_000.0))
    {
        miner.hashrate_ths = Some(rate);
    }
    if let Some(power) = first_num(doc, &["/SUMMARY/0/Power", "/SUMMARY/0/Power_RT"]) {
        miner.power_w = Some(power);
    }
    if let Some(hardware) = num(doc, "/SUMMARY/0/Device Hardware%") {
        miner.asic_pct = Some(hardware);
    }
    if let Some(temp) = first_positive(
        doc,
        &[
            "/SUMMARY/0/Chip Temp Avg",
            "/SUMMARY/0/ChipTempAvg",
            "/SUMMARY/0/Temperature",
            "/SUMMARY/0/temp1",
            "/SUMMARY/0/Temp",
        ],
    ) {
        miner.temp_c = Some(temp);
    }
    if let Some(board) = first_positive(doc, &["/SUMMARY/0/Temperature", "/SUMMARY/0/PCB Temp"]) {
        miner.board_temp = Some(board);
    }
    if let Some(vr) = first_positive(doc, &["/SUMMARY/0/VR Temp", "/SUMMARY/0/vr_temp"]) {
        miner.vr_temp = Some(vr);
    }
    if let Some(fan) = first_positive(doc, &["/SUMMARY/0/Fan Percent", "/SUMMARY/0/Fan Speed"]) {
        miner.fan = Some(fan);
    }
    if let Some(shares) = whole(doc, "/SUMMARY/0/Accepted") {
        if !miner.share_locked {
            miner.shares_accepted = Some(shares);
        }
    }
    if let Some(rejected) = whole(doc, "/SUMMARY/0/Rejected") {
        miner.shares_rejected = Some(rejected);
    }
    if miner.family == "bitmainZEC" {
        miner.best_diff = None;
        miner.best_session = None;
        miner.power_w = None;
    } else {
        commit_best_diff(miner, first_num(doc, &["/SUMMARY/0/Best Share"]));
    }
    if let Some(uptime) = whole(doc, "/SUMMARY/0/Elapsed") {
        miner.uptime_s = Some(uptime);
    }
    let learned = miner.hashrate_ths != saved_hash
        || miner.power_w != saved_power
        || miner.temp_c != saved_temp
        || miner.shares_accepted != saved_shares
        || miner.uptime_s != saved_uptime;
    let answered = text.contains("\"SUMMARY\"") || text.contains("\"Elapsed\"") || text.contains("\"STATUS\"");
    if learned || answered {
        remember(miner);
    } else {
        note_miss(miner);
    }
}

fn blank(host: u8, ip: &str) -> Miner {
    Miner {
        host,
        ip: ip.to_owned(),
        family: "miner",
        name: String::new(),
        model: String::new(),
        mac: String::new(),
        tune: String::new(),
        user: String::new(),
        pass: String::new(),
        token: String::new(),
        detail_step: 0,
        fan_manual: false,
        asic_pct: None,
        hashrate_ths: None,
        power_w: None,
        temp_c: None,
        board_temp: None,
        vr_temp: None,
        fan: None,
        fan_rpm: None,
        fan_pcts: [0.0; 8],
        fan_rpms: [0.0; 8],
        uptime_s: None,
        pool: String::new(),
        pool_user: String::new(),
        shares_accepted: None,
        shares_seen: None,
        share_locked: false,
        shares_rejected: None,
        best_diff: None,
        best_session: None,
        frequency: None,
        default_frequency: None,
        eco_frequency: None,
        voltage: None,
        reachable: false,
        misses: 0,
        chart: Vec::new(),
        chart_dirty: false,
    }
}

fn whole(doc: &JsonDoc, path: &str) -> Option<u64> {
    doc.i64(path)
        .and_then(|value| u64::try_from(value).ok())
        .or_else(|| {
            num(doc, path).and_then(|value| {
                if value > 0.0 {
                    Some(value as u64)
                } else {
                    None
                }
            })
        })
}

fn num(doc: &JsonDoc, path: &str) -> Option<f64> {
    doc.f64(path)
        .or_else(|| doc.i64(path).map(|value| value as f64))
        .or_else(|| {
            doc.str(path).and_then(|value| value.trim().replace(',', "").parse::<f64>().ok())
        })
        .filter(|value| value.is_finite())
}

fn has_reading(miner: &Miner) -> bool {
    miner.hashrate_ths.is_some()
        || miner.power_w.is_some()
        || miner.temp_c.is_some()
        || miner.shares_accepted.is_some()
        || miner.uptime_s.is_some()
}

fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn field_unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn first_num(doc: &JsonDoc, paths: &[&str]) -> Option<f64> {
    paths.iter().find_map(|path| num(doc, path))
}

fn labeled_difficulty(doc: &JsonDoc, paths: &[&str]) -> Option<f64> {
    for path in paths {
        if let Some(value) = num(doc, path).filter(|value| *value > 0.0) {
            return Some(value);
        }
        if let Some(text) = doc.str(path) {
            if let Some(value) = parse_difficulty_label(&text) {
                return Some(value);
            }
        }
    }
    None
}

fn parse_difficulty_label(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (body, scale) = match text.chars().last() {
        Some('T') => (&text[..text.len() - 1], 1_000_000_000_000.0),
        Some('G') => (&text[..text.len() - 1], 1_000_000_000.0),
        Some('M') => (&text[..text.len() - 1], 1_000_000.0),
        Some('K') => (&text[..text.len() - 1], 1_000.0),
        _ => (text, 1.0),
    };
    body.trim().parse::<f64>().ok().filter(|value| value.is_finite() && *value > 0.0).map(|value| value * scale)
}

fn clamp_volume(volume: u8) -> u8 {
    volume.min(100)
}

fn snap_volume(volume: u8) -> u8 {
    match volume {
        0..=12 => 0,
        13..=37 => 25,
        38..=62 => 50,
        63..=87 => 75,
        _ => 100,
    }
}

fn snap_brightness_pct(percent: u8) -> u8 {
    let stepped = u16::from(percent) / 5 * 5;
    stepped.clamp(10, 100) as u8
}

fn first_positive(doc: &JsonDoc, paths: &[&str]) -> Option<f64> {
    paths.iter().find_map(|path| num(doc, path).filter(|value| *value > 0.0))
}

fn ghs_to_ths(ghs: Option<f64>) -> Option<f64> {
    ghs.map(|value| value / 1000.0)
}

fn looks_like_version(value: &str) -> bool {
    let mut dots = 0;
    if value.is_empty() {
        return false;
    }
    for part in value.split('.') {
        if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
        dots += 1;
    }
    dots >= 3
}

fn host_octet(raw: &str) -> Option<(String, u8)> {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix("http://")
        .or_else(|| raw.strip_prefix("https://"))
        .unwrap_or(raw);
    let raw = raw.split('/').next().unwrap_or(raw);
    let raw = raw.split(':').next().unwrap_or(raw).trim();
    let host = raw.rsplit('.').next()?.chars().take_while(|ch| ch.is_ascii_digit()).collect::<String>();
    let host = host.parse::<u8>().ok()?;
    if host == 0 || raw.is_empty() {
        return None;
    }
    Some((raw.to_owned(), host))
}

fn split_v4(ip: &str) -> Option<(String, u8)> {
    let mut parts = ip.split('.');
    let a = parts.next()?;
    let b = parts.next()?;
    let c = parts.next()?;
    let host: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || a.is_empty() || b.is_empty() || c.is_empty() {
        return None;
    }
    Some((format!("{a}.{b}.{c}."), host))
}

fn imported_miners(own: Option<u8>) -> Vec<Miner> {
    let snap = params::current();
    let mut text = String::new();
    for key in ["inventory", "inventory2", "inventory3", "inventory4"] {
        let Some(chunk) = snap.get_str(key) else {
            continue;
        };
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(chunk);
    }
    let mut miners: Vec<Miner> = Vec::new();
    for line in text.lines() {
        let mut fields = line.split('\t');
        let name = field_unescape(fields.next().unwrap_or("").trim());
        let ip = field_unescape(fields.next().unwrap_or("").trim());
        let kind = field_unescape(fields.next().unwrap_or("").trim());
        let mac = field_unescape(fields.next().unwrap_or("").trim());
        let tune = field_unescape(fields.next().unwrap_or("").trim());
        let user = field_unescape(fields.next().unwrap_or("").trim());
        let pass = field_unescape(fields.next().unwrap_or("").trim());
        if ip.is_empty() || kind.eq_ignore_ascii_case("hub") {
            continue;
        }
        let Some((ip, host)) = host_octet(&ip) else {
            continue;
        };
        if host == 0 || Some(host) == own {
            continue;
        }
        let family = family_from_type(&kind);
        if family == "skip" {
            continue;
        }
        if let Some(existing) = miners.iter_mut().find(|miner| miner.ip == ip) {
            let token = if existing.user == user && existing.pass == pass {
                existing.token.clone()
            } else {
                String::new()
            };
            existing.family = family;
            existing.name = name;
            existing.mac = mac;
            existing.tune = tune;
            existing.user = user;
            existing.pass = pass;
            existing.token = token;
            continue;
        }
        let mut miner = blank(host, &ip);
        miner.family = family;
        miner.name = name;
        miner.mac = mac;
        miner.tune = tune;
        miner.user = user;
        miner.pass = pass;
        miners.push(miner);
    }
    miners
}

fn family_from_type(kind: &str) -> &'static str {
    match kind.to_ascii_lowercase().as_str() {
        "harlo" => "harlo",
        "luckyminer" | "lucky" | "luckyaxe" | "luxkyaxe" => "luckyMiner",
        "braiins" | "bos" | "bosminer" | "braiinsos" => "braiins",
        "vnish" => "vnish",
        "zyberos" | "zyber" => "zyberos",
        "canaan" | "avalon" => "canaan",
        "luxos" | "luxor" => "luxos",
        "bitmainzec" | "zec" | "bitmain zec" | "bitmain-zec" | "antminerzec" => "bitmainZEC",
        "futurebit" | "apollo" => "futurebit",
        "cgminer" => "cgminer",
        "hub" => "skip",
        "bitaxe" | "nerdq" | "nerdminer" | "nerdqaxe" | "nerdqaxeplus" | "nmminer" | "nmaxe" | "nmmaxe" | "bitdsk" | "octaxe" | "hammer" | "thor" | "nexusl1" | "nexus" => "bitaxe",
        _ => "bitaxe",
    }
}

enum BraiinsTuneKind {
    Hashrate,
    Power,
}

struct BraiinsTune {
    label: String,
    kind: BraiinsTuneKind,
    amount: u32,
}

fn braiins_tunes(text: &str) -> Vec<BraiinsTune> {
    let mut tunes = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, spec) = part.split_once(':').unwrap_or(("", part));
        let spec = spec.trim();
        let (kind, digits) = if let Some(rest) = spec.strip_prefix('H').or_else(|| spec.strip_prefix('h')) {
            (BraiinsTuneKind::Hashrate, rest)
        } else if let Some(rest) = spec.strip_prefix('P').or_else(|| spec.strip_prefix('p')) {
            (BraiinsTuneKind::Power, rest)
        } else {
            continue;
        };
        let Ok(amount) = digits.parse::<u32>() else {
            continue;
        };
        if amount == 0 {
            continue;
        }
        let label = if name.is_empty() {
            match kind {
                BraiinsTuneKind::Hashrate => format!("{amount} TH"),
                BraiinsTuneKind::Power => format!("{amount} W"),
            }
        } else {
            match kind {
                BraiinsTuneKind::Hashrate => format!("{name} {amount} TH"),
                BraiinsTuneKind::Power => format!("{name} {amount} W"),
            }
        };
        tunes.push(BraiinsTune { label, kind, amount });
    }
    tunes
}

fn family_label(family: &str) -> &str {
    match family {
        "bitaxe" => "Bitaxe",
        "harlo" => "Harlo",
        "braiins" => "Braiins",
        "vnish" => "VNish",
        "luckyMiner" => "Lucky Miner",
        "zyberos" => "ZyberOS",
        "futurebit" => "FutureBit",
        "luxos" => "LuxOS",
        "canaan" => "Canaan",
        "bitmainZEC" => "Bitmain ZEC",
        "cgminer" => "CGMiner",
        _ => "Miner",
    }
}

fn miner_title(miner: &Miner) -> String {
    if !miner.name.is_empty() && !looks_like_version(&miner.name) {
        miner.name.clone()
    } else if !miner.model.is_empty() {
        miner.model.clone()
    } else {
        family_label(miner.family).to_owned()
    }
}

fn status_color(miner: &Miner) -> Color {
    if !miner.reachable {
        RED
    } else if miner.hashrate_ths.unwrap_or(0.0) <= 0.0 {
        AMBER
    } else {
        GREEN
    }
}

fn wordmark(size: u32) -> Node {
    row(
        props!(gap: 0.0, cross_align: CrossAlign::Center),
        [
            text(
                "Hash",
                style!(size: size, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: WHITE, line_height: 1.0, valign: VerticalAlign::Center),
            ),
            text(
                "Watcher",
                style!(size: size, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: EMERALD, line_height: 1.0, valign: VerticalAlign::Center),
            ),
        ],
    )
}

struct FleetNumbers {
    sha: f64,
    zec: f64,
    shares: String,
    temp: String,
    eff: String,
    power: String,
    online: String,
}

fn fleet_numbers(app: &App) -> FleetNumbers {
    let mut sha = 0.0;
    let mut sha_power = 0.0;
    let mut zec = 0.0;
    let mut power = 0.0;
    for miner in &app.miners {
        let rate = miner.hashrate_ths.unwrap_or(0.0);
        let watts = miner.power_w.unwrap_or(0.0);
        power += watts;
        if miner.family == "bitmainZEC" {
            zec += rate;
        } else {
            sha += rate;
            sha_power += watts;
        }
    }
    let shares: u64 = app.miners.iter().filter_map(|miner| miner.shares_accepted).sum();
    let temps: Vec<f64> = app.miners.iter().filter_map(|miner| miner.temp_c).collect();
    let avg = if temps.is_empty() {
        None
    } else {
        Some(temps.iter().sum::<f64>() / temps.len() as f64)
    };
    let efficiency = if sha > 0.0 && sha_power > 0.0 {
        Some(sha_power / sha)
    } else {
        None
    };
    let online = app
        .miners
        .iter()
        .filter(|miner| miner.reachable && miner.hashrate_ths.unwrap_or(0.0) > 0.0)
        .count();
    FleetNumbers {
        sha,
        zec,
        shares: count_text(Some(shares).filter(|value| *value > 0)),
        temp: format_temp(avg),
        eff: format_efficiency(efficiency),
        power: format_power(Some(power).filter(|value| *value > 0.0)),
        online: format!("{online}/{}", app.miners.len()),
    }
}

fn fleet_hash_lines(sha: f64, zec: f64) -> (String, Option<String>) {
    let sha_text = (sha > 0.0).then(|| format_hashrate(Some(sha)));
    let zec_text = (zec > 0.0).then(|| format!("{zec:.0} KSol/s"));
    match (sha_text, zec_text) {
        (Some(sha_text), zec_text) => (sha_text, zec_text),
        (None, Some(zec_text)) => (zec_text, None),
        (None, None) => ("—".to_owned(), None),
    }
}

fn hash_summary_cell(value: &str, zec: Option<&str>, width: f32) -> Node {
    let mut rows = vec![
        text("Hashrate", style!(size: 12, weight: FontWeight::SEMIBOLD, color: LABEL, line_height: 1.0)),
        text(value, style!(size: 16, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
    ];
    if let Some(zec) = zec {
        rows.push(text(zec, style!(size: 11, weight: FontWeight::SEMIBOLD, color: LABEL, line_height: 1.0)));
    }
    col(props!(width: width, gap: 1.0), rows)
}

fn summary_cell(title: &str, value: &str, width: f32) -> Node {
    col(
        props!(width: width, gap: 1.0),
        [
            text(title, style!(size: 12, weight: FontWeight::SEMIBOLD, color: LABEL, line_height: 1.0)),
            text(value, style!(size: 16, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
        ],
    )
}

fn summary_totals_row(app: &App, width: f32) -> Node {
    let numbers = fleet_numbers(app);
    let hashing = app
        .miners
        .iter()
        .filter(|miner| miner.reachable && miner.hashrate_ths.unwrap_or(0.0) > 0.0)
        .count();
    let total = app.miners.len();
    let live = if total == 0 {
        "—".to_owned()
    } else if hashing == total {
        if total == 1 { "1 miner".to_owned() } else { format!("{total} miners") }
    } else {
        format!("{hashing} of {total} live")
    };
    let (hash, zec_hash) = fleet_hash_lines(numbers.sha, numbers.zec);
    let cell_w = ((width - 96.0) / 4.0).max(70.0);
    row(
        props!(height: 52.0, gap: 8.0, cross_align: CrossAlign::Center, width: width),
        [
            text("Totals", style!(size: 16, weight: FontWeight::BOLD, color: EMERALD, line_height: 1.0)),
            summary_cell("Pool", &live, cell_w),
            hash_summary_cell(&hash, zec_hash.as_deref(), cell_w),
            summary_cell("Power", &numbers.power, cell_w),
            summary_cell("Temperature", &numbers.temp, cell_w),
        ],
    )
}

fn hashrate_parts(value: Option<f64>) -> (String, String) {
    let text = format_hashrate(value);
    match text.rsplit_once(' ') {
        Some((number, unit)) => (number.to_owned(), unit.to_owned()),
        None => (text, String::new()),
    }
}

fn temp_color(temp: Option<f64>, family: &str) -> Color {
    let (warning, danger) = match family {
        "canaan" => (95.0, 96.0),
        "bitmainZEC" => (85.0, 95.0),
        "cgminer" => (70.0, 85.0),
        "luxos" => (75.0, 85.0),
        _ => (70.0, 75.0),
    };
    match temp {
        Some(value) if value >= danger => RED,
        Some(value) if value >= warning => AMBER,
        Some(value) if value > 0.0 => EMERALD,
        _ => MUTED,
    }
}

fn count_text(value: Option<u64>) -> String {
    let Some(value) = value else {
        return "—".to_owned();
    };
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped.chars().rev().collect()
}

fn canaan_uses_worklevel(miner: &Miner) -> bool {
    format!("{} {}", miner.model, miner.name).to_ascii_lowercase().contains("nano")
}

fn canaan_is_mini(miner: &Miner) -> bool {
    format!("{} {}", miner.model, miner.name).to_ascii_lowercase().contains("mini")
}

fn canaan_mode_label(miner: &Miner, mode: i32) -> &'static str {
    canaan_mode_buttons(miner)
        .into_iter()
        .find(|(id, _)| id.ends_with(&mode.to_string()))
        .map(|(_, label)| label)
        .unwrap_or("Mode")
}

fn canaan_mode_buttons(miner: &Miner) -> Vec<(&'static str, &'static str)> {
    if canaan_uses_worklevel(miner) {
        vec![("mode-0", "Low"), ("mode-1", "Mid"), ("mode-2", "High")]
    } else if canaan_is_mini(miner) {
        vec![("mode-0", "Heater"), ("mode-1", "Mining"), ("mode-2", "Night")]
    } else {
        vec![("mode-0", "Eco"), ("mode-1", "Standard"), ("mode-2", "Super")]
    }
}

fn canaan_mode_hint(miner: &Miner) -> &'static str {
    if canaan_uses_worklevel(miner) {
        "Nano3 uses Low, Mid, and High."
    } else if canaan_is_mini(miner) {
        "Mini3 uses Heater, Mining, and Night."
    } else {
        "Avalon uses Eco, Standard, and Super."
    }
}

fn chart_window(samples: &[ChartSample], start: i64) -> Vec<ChartSample> {
    let mut window: Vec<ChartSample> = samples.iter().copied().filter(|sample| sample.at >= start).collect();
    const MAX_POINTS: usize = 280;
    if window.len() > MAX_POINTS {
        let stride = window.len().div_ceil(MAX_POINTS).max(1);
        let mut thinned = Vec::new();
        for (index, sample) in window.iter().enumerate() {
            if index % stride == 0 || index + 1 == window.len() {
                thinned.push(*sample);
            }
        }
        window = thinned;
    }
    window
}

struct ChartStats {
    min: f64,
    max: f64,
    avg: f64,
    count: usize,
}

fn chart_plot(samples: &[ChartSample], now: i64, span: i64, width: f32, height: f32, axes: bool) -> (Vec<Draw>, ChartStats) {
    let start = now.saturating_sub(span.max(1));
    let window = chart_window(samples, start);
    let stats = if window.is_empty() {
        ChartStats { min: 0.0, max: 0.0, avg: 0.0, count: 0 }
    } else {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        let mut sum = 0.0;
        for sample in &window {
            let value = f64::from(sample.hash);
            min = min.min(value);
            max = max.max(value);
            sum += value;
        }
        ChartStats {
            min,
            max,
            avg: sum / window.len() as f64,
            count: window.len(),
        }
    };
    let left = if axes { 92.0 } else { 0.0 };
    let bottom = if axes { 26.0 } else { 0.0 };
    let plot_w = (width - left).max(20.0);
    let plot_h = (height - bottom).max(20.0);
    let mut draws = Vec::new();
    if axes {
        for step in 0..4 {
            let y = plot_h * (step as f32 / 3.0);
            draws.push(path!(
                vec![(left, y), (left + plot_w, y)],
                stroke: 1.0,
                color: Color::from_rgba(255, 255, 255, 50)
            ));
        }
    }
    if window.len() < 2 {
        return (draws, stats);
    }
    let pad = ((stats.max - stats.min) * 0.12).max(0.05);
    let low = stats.min - pad;
    let high = stats.max + pad;
    let scale = (high - low).max(0.001);
    let mut mapped = Vec::with_capacity(window.len());
    for sample in &window {
        let x = left + (sample.at.saturating_sub(start) as f32 / span as f32).clamp(0.0, 1.0) * plot_w;
        let y = plot_h - ((f64::from(sample.hash) - low) / scale) as f32 * (plot_h - 8.0) - 4.0;
        mapped.push((sample.at, x, y));
    }
    let break_after = (span / 18).max(120);
    let mut segment: Vec<(f32, f32)> = Vec::new();
    let mut flush = |segment: &mut Vec<(f32, f32)>, draws: &mut Vec<Draw>| {
        if segment.len() < 2 {
            segment.clear();
            return;
        }
        let mut fill_pts = segment.clone();
        let first = segment[0];
        let last = *segment.last().unwrap_or(&first);
        fill_pts.push((last.0, plot_h));
        fill_pts.push((first.0, plot_h));
        draws.push(fill!(
            fill_pts,
            color: Color::from_rgba(0, 230, 120, 110)
        ));
        let smooth = segment.len() >= 4;
        let line = Color::from_rgb(120, 255, 170);
        draws.push(if smooth {
            path!(segment.clone(), stroke: 4.0, color: line, smooth)
        } else {
            path!(segment.clone(), stroke: 4.0, color: line)
        });
        segment.clear();
    };
    for (index, point) in mapped.iter().enumerate() {
        if index > 0 && point.0.saturating_sub(mapped[index - 1].0) > break_after {
            flush(&mut segment, &mut draws);
        }
        segment.push((point.1, point.2));
    }
    flush(&mut segment, &mut draws);
    if axes {
        draws.push(Draw::text(
            0.0,
            0.0,
            format_hashrate(Some(stats.max)),
            style!(size: 14, weight: FontWeight::SEMIBOLD, color: WHITE),
        ));
        draws.push(Draw::text(
            0.0,
            (plot_h - 18.0).max(0.0),
            format_hashrate(Some(stats.min)),
            style!(size: 14, weight: FontWeight::SEMIBOLD, color: WHITE),
        ));
        let marks = [
            (start, left, TextAlign::Left),
            (start + span / 2, left + plot_w * 0.5, TextAlign::Center),
            (now, left + plot_w, TextAlign::Right),
        ];
        for (mark, x, align) in marks {
            draws.push(Draw::text(
                x,
                plot_h + 4.0,
                chart_clock(mark, span >= 12 * 60 * 60),
                style!(size: 14, weight: FontWeight::SEMIBOLD, color: WHITE, align: align),
            ));
        }
    }
    (draws, stats)
}

fn chart_clock(unix: i64, with_day: bool) -> String {
    let snap = bmc_wasm_sdk::system::current();
    let tz = snap.timezone().unwrap_or("Etc/GMT");
    let local = bmc_wasm_sdk::calendar::tz_convert(unix, tz).unwrap_or_else(|| SystemTime { unix_secs: unix }.utc());
    let hour = if local.hour % 12 == 0 { 12 } else { local.hour % 12 };
    let suffix = if local.hour < 12 { "AM" } else { "PM" };
    if with_day {
        format!("{} {hour}:{:02} {suffix}", local.month_short(), local.minute)
    } else {
        format!("{hour}:{:02} {suffix}", local.minute)
    }
}

fn hash_metric(sha: &str, zec: Option<&str>, width: f32, height: f32) -> Node {
    let mut rows = vec![
        text(
            "Total Hash",
            style!(
                size: 24,
                weight: FontWeight::SEMIBOLD,
                color: LABEL,
                line_height: 1.0,
                align: TextAlign::Center,
            ),
        ),
        text(
            sha,
            style!(
                size: if zec.is_some() { 26 } else { 32 },
                weight: FontWeight::BOLD,
                family: FontFamily::DeckSans,
                color: EMERALD,
                line_height: 1.0,
                align: TextAlign::Center,
                text_overflow: TextOverflow::Ellipsis,
                max_width: (width - 8.0).max(20.0) as u32,
            ),
        ),
    ];
    if let Some(zec) = zec {
        rows.push(text(
            zec,
            style!(
                size: 22,
                weight: FontWeight::SEMIBOLD,
                family: FontFamily::DeckSans,
                color: EMERALD,
                line_height: 1.0,
                align: TextAlign::Center,
                text_overflow: TextOverflow::Ellipsis,
                max_width: (width - 8.0).max(20.0) as u32,
            ),
        ));
    }
    col(
        props!(
            width: width,
            height: height,
            padding: 4.0,
            gap: 0.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        rows,
    )
}

fn metric_widget(title: &str, value: &str, value_color: Color, width: f32, height: f32) -> Node {
    col(
        props!(
            width: width,
            height: height,
            padding: 4.0,
            gap: 0.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                title,
                style!(
                    size: 24,
                    weight: FontWeight::SEMIBOLD,
                    color: LABEL,
                    line_height: 1.0,
                    align: TextAlign::Center,
                ),
            ),
            text(
                value,
                style!(
                    size: 32,
                    weight: FontWeight::BOLD,
                    family: FontFamily::DeckSans,
                    color: value_color,
                    line_height: 1.0,
                    align: TextAlign::Center,
                    text_overflow: TextOverflow::Ellipsis,
                    max_width: (width - 8.0).max(20.0) as u32,
                ),
            ),
        ],
    )
}

fn screen_h(height: f32) -> f32 {
    if height < 100.0 { 480.0 } else { height }
}

fn fleet_list_height(height: f32) -> f32 {
    (screen_h(height) - FLEET_HEADER_H - FLEET_PAD - FLEET_METRICS_H - FLEET_GAP - FLEET_PAD).max(120.0)
}

fn family_rank(family: &str) -> u8 {
    match family {
        "bitaxe" => 0,
        "luckyMiner" => 1,
        "canaan" => 2,
        "braiins" => 3,
        "vnish" => 4,
        "harlo" => 5,
        "bitmainZEC" => 6,
        "luxos" => 7,
        "futurebit" => 8,
        "zyberos" => 9,
        "cgminer" => 10,
        _ => 11,
    }
}

fn fleet_card_width(width: f32) -> f32 {
    let inner = width - FLEET_PAD * 2.0;
    let gaps = FLEET_GAP * (FLEET_COLUMNS as f32 - 1.0);
    ((inner - gaps) / FLEET_COLUMNS as f32).max(40.0)
}

fn neural_layout(count: usize, width: f32, height: f32, spin: f32) -> (f32, f32, f32, Vec<f32>, Vec<(f32, f32)>) {
    let count = count.max(1);
    let cx = width * 0.5;
    let cy = height * 0.50;
    let max_rx = (width * 0.5 - 64.0).max(90.0);
    let max_ry = (height * 0.5 - 36.0).max(50.0);
    let rx = (width * 0.40).min(max_rx);
    let ry = (height * 0.28).min(max_ry);
    let outer = rx.max(ry);
    let mut angles = Vec::with_capacity(count);
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        let angle = spin
            + if count == 2 {
                index as f32 * 3.14159
            } else {
                6.28318 * (index as f32 + 0.5) / count as f32 - 1.5708
            };
        let jitter = (index as f32 * 2.7 + 0.5).sin();
        let point_rx = rx + jitter * rx * 0.04;
        let point_ry = ry + jitter * ry * 0.04;
        angles.push(angle);
        points.push((cx + angle.cos() * point_rx, cy + angle.sin() * point_ry));
    }
    (cx, cy, outer, angles, points)
}

fn neural_points(count: usize, width: f32, height: f32, spin: f32) -> Vec<(f32, f32)> {
    neural_layout(count, width, height, spin).4
}

fn neural_globe_size(miner: &Miner, count: usize, max_hash: f64) -> f32 {
    let (min_d, max_d) = if count <= 4 {
        (128.0, 200.0)
    } else if count <= 8 {
        (108.0, 176.0)
    } else if count <= 14 {
        (92.0, 152.0)
    } else {
        (80.0, 128.0)
    };
    if !miner.reachable || miner.hashrate_ths.unwrap_or(0.0) <= 0.0 || max_hash <= 0.0 {
        return min_d * 0.82;
    }
    let normalised = (miner.hashrate_ths.unwrap_or(0.0) / max_hash).clamp(0.0, 1.0);
    let scaled = normalised.sqrt();
    min_d + (max_d - min_d) * scaled as f32
}

fn neural_canvas_size(width: f32, height: f32) -> (f32, f32) {
    (width.max(40.0), screen_h(height).max(140.0))
}

fn neural_node_bitmap(miner: &Miner) -> &'static Bitmap {
    if miner.reachable && miner.hashrate_ths.unwrap_or(0.0) > 0.0 {
        &NODE_IMG
    } else {
        &NODE_IDLE
    }
}

fn miner_flagged(app: &App, miner: &Miner) -> bool {
    app.share_pulse
        && app.share_flash_ms > 0
        && app.share_ips.iter().any(|ip| ip == &miner.ip)
}

fn share_flash_alpha(remaining_ms: u32) -> u8 {
    if remaining_ms == 0 || remaining_ms > SHARE_FLASH_MS {
        return 0;
    }
    let elapsed = (SHARE_FLASH_MS - remaining_ms) as f32 / SHARE_FLASH_MS as f32;
    ((elapsed * std::f32::consts::PI).sin() * 255.0) as u8
}

fn neural_exit_hit(x: f32, y: f32, width: f32) -> bool {
    x >= width - 110.0 && y <= 130.0
}

fn neural_board(app: &App, width: f32, height: f32) -> Node {
    let (canvas_w, canvas_h) = neural_canvas_size(width, height);
    let count = app.miners.len().max(1);
    let (cx, cy, _outer, _angles, points) = neural_layout(app.miners.len(), canvas_w, canvas_h, app.neural_spin_drawn);
    let max_hash = app
        .miners
        .iter()
        .filter_map(|miner| miner.hashrate_ths)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let nucleus = (canvas_w.min(canvas_h) * 0.30).clamp(96.0, 150.0);
    let numbers = fleet_numbers(app);
    let (hash, zec_hash) = fleet_hash_lines(numbers.sha, numbers.zec);
    let share_color = if app.share_flash_ms > 0 { SHARE_ORANGE } else { WHITE };
    let mut order: Vec<usize> = (0..app.miners.len()).collect();
    order.sort_by(|&left, &right| points[left].1.total_cmp(&points[right].1));
    let back = order.first().map(|index| points[*index].1).unwrap_or(cy);
    let front = order.last().map(|index| points[*index].1).unwrap_or(cy);
    let span = (front - back).max(1.0);
    let mut draws = Vec::new();
    let mut drew_nucleus = false;
    for index in order {
        let miner = &app.miners[index];
        let (x, y) = points[index];
        if !drew_nucleus && y >= cy {
            draws.push(Draw::bitmap(cx - nucleus * 0.5, cy - nucleus * 0.5, nucleus, nucleus, &NUCLEUS_IMG));
            drew_nucleus = true;
        }
        let depth = ((y - back) / span).clamp(0.0, 1.0);
        let flagged = miner_flagged(app, miner);
        let mut globe = neural_globe_size(miner, count, max_hash) * (0.92 + 0.08 * depth);
        if flagged {
            let alpha = share_flash_alpha(app.share_flash_ms);
            globe *= 1.0 + 0.08 * (alpha as f32 / 255.0);
            draws.push(Draw::circle(
                x,
                y,
                globe * 0.5 + 3.0,
                Color::from_rgba(255, 159, 10, alpha),
            ));
        }
        let graphic = neural_node_bitmap(miner);
        draws.push(Draw::bitmap(x - globe * 0.5, y - globe * 0.5, globe, globe, graphic));
        if app.label_mode == 0 {
            let label = if miner.name.is_empty() { miner.ip.clone() } else { miner.name.clone() };
            draws.push(Draw::text(
                x,
                y + globe * 0.5 + 12.0,
                label,
                style!(size: 24, weight: FontWeight::SEMIBOLD, color: WHITE, align: TextAlign::Center),
            ));
        } else if app.label_mode == 1 {
            draws.push(Draw::text(
                x,
                y + globe * 0.5 + 22.0,
                display_hashrate(miner),
                style!(size: 22, weight: FontWeight::BOLD, color: EMERALD, align: TextAlign::Center),
            ));
        }
    }
    if !drew_nucleus {
        draws.push(Draw::bitmap(cx - nucleus * 0.5, cy - nucleus * 0.5, nucleus, nucleus, &NUCLEUS_IMG));
    }
    draws.push(Draw::text(
        24.0,
        34.0,
        hash,
        style!(size: 40, weight: FontWeight::BOLD, color: EMERALD, align: TextAlign::Left, valign: VerticalAlign::Center),
    ));
    if let Some(zec) = zec_hash {
        draws.push(Draw::text(
            24.0,
            64.0,
            zec,
            style!(size: 18, weight: FontWeight::SEMIBOLD, color: LABEL, align: TextAlign::Left, valign: VerticalAlign::Center),
        ));
    }
    draws.push(Draw::text(
        cx,
        34.0,
        clock_text(),
        style!(size: 40, weight: FontWeight::BOLD, color: WHITE, align: TextAlign::Center, valign: VerticalAlign::Center),
    ));
    draws.push(Draw::text(
        canvas_w - 24.0,
        34.0,
        numbers.shares,
        style!(size: 40, weight: FontWeight::BOLD, color: share_color, align: TextAlign::Right, valign: VerticalAlign::Center),
    ));
    if app.neural_hint_ms > 0 {
        let alpha = if app.neural_hint_ms > 450 {
            255
        } else {
            (app.neural_hint_ms.saturating_mul(255) / 450) as u8
        };
        let mark_x = canvas_w - 56.0;
        let mark_y = 86.0;
        draws.push(Draw::circle(mark_x, mark_y, 30.0, Color::from_rgba(0, 0, 0, alpha / 2)));
        draws.push(Draw::text(
            mark_x,
            mark_y,
            "X",
            style!(
                size: 48,
                weight: FontWeight::BOLD,
                color: Color::from_rgba(255, 255, 255, alpha),
                align: TextAlign::Center,
                valign: VerticalAlign::Center,
            ),
        ));
    }
    touchable(
        "neural-hit",
        props!(width: canvas_w, height: canvas_h, background: NEURAL_FILL),
        draws,
    )
}

fn with_theme(width: f32, height: f32, content: Node) -> Node {
    col(
        props!(width: width, height: height, background: TRANSPARENT),
        [
            canvas(
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                [Draw::bitmap(0.0, 0.0, width, height, &EMERALD_BG)],
            ),
            content,
        ],
    )
}

fn gear_button() -> Node {
    touchable(
        "open-settings",
        props!(width: 48.0, height: 48.0),
        [Draw::bitmap(2.0, 2.0, 44.0, 44.0, &GEAR_ICON)],
    )
}

fn back_button(id: &str) -> Node {
    col(
        props!(
            height: 52.0,
            background: Color::from_hex(0x1C_2E_24),
            border_radius: 10.0,
            border_width: 1.5,
            border_color: Color::from_hex(0x3E_6B_50),
            padding: 16.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                "Back",
                style!(
                    size: 22,
                    weight: FontWeight::BOLD,
                    color: WHITE,
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                ),
            ),
            touchable(
                id,
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn display_hashrate(miner: &Miner) -> String {
    if miner.family == "bitmainZEC" {
        match miner.hashrate_ths {
            Some(value) if value > 0.0 => format!("{value:.2} KSol/s"),
            _ => "—".to_owned(),
        }
    } else {
        format_hashrate(miner.hashrate_ths)
    }
}

fn url_query(value: &str) -> String {
    let mut out = String::new();
    for byte in value.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(byte as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn device_fahrenheit() -> bool {
    let snap = bmc_wasm_sdk::system::current();
    matches!(snap.temperature_unit(), Some(bmc_wasm_sdk::system::TemperatureUnit::Fahrenheit))
        || matches!(snap.unit_system(), Some(bmc_wasm_sdk::system::UnitSystem::Imperial))
}

fn local_now() -> LocalDateTime {
    let now = SystemTime::now();
    let snap = bmc_wasm_sdk::system::current();
    let tz = snap.timezone().unwrap_or("Etc/GMT");
    bmc_wasm_sdk::calendar::tz_convert(now.unix_secs, tz).unwrap_or_else(|| now.utc())
}

fn clock_glyph(ch: char) -> Option<(&'static Bitmap, f32)> {
    match ch {
        '0' => Some((&CLOCK_0, 76.0)),
        '1' => Some((&CLOCK_1, 52.0)),
        '2' => Some((&CLOCK_2, 78.0)),
        '3' => Some((&CLOCK_3, 78.0)),
        '4' => Some((&CLOCK_4, 84.0)),
        '5' => Some((&CLOCK_5, 79.0)),
        '6' => Some((&CLOCK_6, 78.0)),
        '7' => Some((&CLOCK_7, 76.0)),
        '8' => Some((&CLOCK_8, 76.0)),
        '9' => Some((&CLOCK_9, 78.0)),
        ':' => Some((&CLOCK_COLON, 24.0)),
        'A' => Some((&CLOCK_A, 113.0)),
        'M' => Some((&CLOCK_M, 110.0)),
        'P' => Some((&CLOCK_P, 88.0)),
        _ => None,
    }
}

fn clock_face() -> Node {
    let gap = 4.0;
    let mut cursor = 0.0_f32;
    let mut placed = Vec::new();
    for ch in clock_text().chars() {
        if ch == ' ' {
            cursor += 28.0;
            continue;
        }
        let Some((glyph, width)) = clock_glyph(ch) else {
            continue;
        };
        placed.push((cursor, glyph, width));
        cursor += width + gap;
    }
    if cursor > gap {
        cursor -= gap;
    }
    let draws = placed
        .into_iter()
        .map(|(x, glyph, width)| Draw::bitmap(x, 0.0, width, CLOCK_H, glyph))
        .collect::<Vec<_>>();
    canvas(props!(width: cursor.max(1.0), height: CLOCK_H), draws)
}

fn clock_text() -> String {
    let local = local_now();
    let hour = if local.hour % 12 == 0 { 12 } else { local.hour % 12 };
    let suffix = if local.hour < 12 { "AM" } else { "PM" };
    format!("{hour}:{:02} {suffix}", local.minute)
}

fn date_text() -> String {
    let local = local_now();
    format!("{} {}, {}", local.month_short(), local.day, local.year)
}

fn weather_temp(celsius: f64, fahrenheit: bool) -> String {
    if fahrenheit {
        format!("{:.0}°F", celsius * 9.0 / 5.0 + 32.0)
    } else {
        format!("{:.0}°C", celsius)
    }
}

fn wind_compass(deg: f64) -> &'static str {
    const DIRS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let index = ((deg + 22.5) / 45.0).floor() as i64;
    DIRS[index.rem_euclid(8) as usize]
}

fn weather_wind(kmh: f64, deg: Option<f64>, fahrenheit: bool) -> String {
    let speed = if fahrenheit {
        format!("{:.0} mph", kmh * 0.621371)
    } else {
        format!("{:.0} km/h", kmh)
    };
    match deg {
        Some(deg) if deg.is_finite() => format!("{speed} {}", wind_compass(deg)),
        _ => speed,
    }
}

fn weather_description(code: i64) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 | 77 => "Snow",
        80 | 81 | 82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 | 96 | 99 => "Thunderstorm",
        _ => "Cloudy",
    }
}

fn mix_color(from: (u8, u8, u8), to: (u8, u8, u8), t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let ch = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
    Color::from_rgb(ch(from.0, to.0), ch(from.1, to.1), ch(from.2, to.2))
}

fn solid_rect(x: f32, y: f32, w: f32, h: f32, color: Color) -> Draw {
    fill!(vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)], color: color)
}

fn disc(cx: f32, cy: f32, radius: f32, color: Color) -> Draw {
    let points = (0..18)
        .map(|index| {
            let angle = index as f32 / 18.0 * 6.28318;
            (cx + angle.cos() * radius, cy + angle.sin() * radius)
        })
        .collect::<Vec<_>>();
    fill!(points, color: color)
}

fn gradient_bands(width: f32, height: f32, stops: [(u8, u8, u8); 3]) -> Vec<Draw> {
    let bands = 28;
    (0..bands)
        .map(|index| {
            let t = index as f32 / (bands as f32 - 1.0);
            let color = if t < 0.5 {
                mix_color(stops[0], stops[1], t * 2.0)
            } else {
                mix_color(stops[1], stops[2], (t - 0.5) * 2.0)
            };
            let y = height * index as f32 / bands as f32;
            solid_rect(0.0, y, width, height / bands as f32 + 1.0, color)
        })
        .collect()
}

fn weather_stops(code: i64, is_day: bool) -> [(u8, u8, u8); 3] {
    let rain = (51..=67).contains(&code) || (80..=82).contains(&code);
    let snow = (71..=77).contains(&code) || code == 85 || code == 86;
    let thunder = code >= 95;
    let fog = code == 45 || code == 48;
    if !is_day {
        return if thunder {
            [(18, 23, 35), (26, 36, 53), (8, 12, 20)]
        } else if snow {
            [(30, 44, 64), (42, 63, 89), (14, 23, 38)]
        } else if rain {
            [(26, 34, 50), (34, 51, 75), (11, 19, 32)]
        } else if fog {
            [(41, 49, 69), (53, 65, 90), (26, 33, 49)]
        } else if code == 3 {
            [(34, 42, 57), (42, 53, 72), (17, 25, 39)]
        } else if code == 1 || code == 2 {
            [(18, 26, 46), (26, 39, 69), (10, 18, 34)]
        } else {
            [(9, 17, 30), (16, 32, 58), (4, 9, 19)]
        };
    }
    if thunder {
        [(75, 85, 104), (45, 57, 76), (19, 30, 45)]
    } else if snow {
        [(176, 199, 221), (125, 157, 189), (68, 102, 132)]
    } else if rain {
        [(92, 111, 141), (60, 84, 115), (28, 48, 74)]
    } else if fog {
        [(157, 178, 200), (115, 139, 167), (72, 94, 124)]
    } else if code == 3 {
        [(129, 150, 180), (86, 108, 141), (44, 67, 98)]
    } else if code == 1 || code == 2 {
        [(126, 196, 255), (74, 143, 216), (30, 82, 142)]
    } else {
        [(92, 183, 255), (46, 123, 209), (23, 68, 132)]
    }
}

enum SkyKind {
    Clear,
    Partly,
    Overcast,
    Fog,
    Rain,
    Snow,
    Thunder,
}

fn sky_kind(code: i64) -> SkyKind {
    if code >= 95 {
        SkyKind::Thunder
    } else if (71..=77).contains(&code) || code == 85 || code == 86 {
        SkyKind::Snow
    } else if (51..=67).contains(&code) || (80..=82).contains(&code) {
        SkyKind::Rain
    } else if code == 45 || code == 48 {
        SkyKind::Fog
    } else if code == 3 {
        SkyKind::Overcast
    } else if code == 1 || code == 2 {
        SkyKind::Partly
    } else {
        SkyKind::Clear
    }
}

fn push_cloud(draws: &mut Vec<Draw>, x: f32, y: f32, size: f32, color: Color) {
    draws.push(disc(x, y + size * 0.08, size * 0.28, color));
    draws.push(disc(x + size * 0.26, y, size * 0.36, color));
    draws.push(disc(x + size * 0.52, y + size * 0.1, size * 0.26, color));
}

fn push_orb(draws: &mut Vec<Draw>, cx: f32, cy: f32, radius: f32, is_day: bool) {
    let glow = if is_day {
        Color::from_rgba(255, 224, 145, 90)
    } else {
        Color::from_rgba(199, 214, 255, 70)
    };
    let body = if is_day {
        Color::from_rgba(255, 227, 140, 235)
    } else {
        Color::from_rgba(221, 230, 255, 220)
    };
    draws.push(disc(cx, cy, radius * 1.7, glow));
    if is_day {
        for index in 0..8 {
            let angle = index as f32 / 8.0 * 6.28318;
            let dx = angle.cos();
            let dy = angle.sin();
            let px = -dy * radius * 0.08;
            let py = dx * radius * 0.08;
            let inner = radius * 1.15;
            let outer = radius * 1.55;
            draws.push(fill!(
                vec![
                    (cx + dx * inner + px, cy + dy * inner + py),
                    (cx + dx * outer + px, cy + dy * outer + py),
                    (cx + dx * outer - px, cy + dy * outer - py),
                    (cx + dx * inner - px, cy + dy * inner - py),
                ],
                color: body
            ));
        }
    }
    draws.push(disc(cx, cy, radius, body));
    if !is_day {
        let shade = Color::from_rgba(176, 194, 230, 150);
        draws.push(disc(cx - radius * 0.28, cy - radius * 0.16, radius * 0.18, shade));
        draws.push(disc(cx + radius * 0.24, cy + radius * 0.18, radius * 0.11, shade));
    }
}

fn push_stars(draws: &mut Vec<Draw>, width: f32, height: f32) {
    const STARS: [(f32, f32, f32); 12] = [
        (0.08, 0.10, 2.2),
        (0.18, 0.28, 1.4),
        (0.30, 0.08, 1.8),
        (0.42, 0.20, 1.2),
        (0.54, 0.06, 2.0),
        (0.66, 0.18, 1.3),
        (0.10, 0.42, 1.5),
        (0.26, 0.38, 1.1),
        (0.48, 0.34, 1.6),
        (0.72, 0.40, 1.2),
        (0.04, 0.24, 1.7),
        (0.36, 0.48, 1.3),
    ];
    for (x, y, radius) in STARS {
        draws.push(disc(
            width * x,
            height * y,
            radius,
            Color::from_rgba(255, 248, 230, 210),
        ));
    }
}

fn push_rain(draws: &mut Vec<Draw>, spots: &[(f32, f32, f32)]) {
    let color = Color::from_rgba(175, 226, 255, 200);
    for (x, y, h) in spots {
        draws.push(fill!(
            vec![(*x, *y), (*x + 6.0, *y), (*x - 8.0, *y + *h), (*x - 14.0, *y + *h)],
            color: color
        ));
    }
}

fn push_flakes(draws: &mut Vec<Draw>, spots: &[(f32, f32, f32)]) {
    let color = Color::from_rgba(231, 245, 255, 230);
    for (x, y, size) in spots {
        draws.push(disc(*x, *y, *size * 0.22, color));
        for index in 0..3 {
            let angle = index as f32 / 3.0 * 3.14159;
            let dx = angle.cos() * *size;
            let dy = angle.sin() * *size;
            let px = -angle.sin() * (*size * 0.08);
            let py = angle.cos() * (*size * 0.08);
            draws.push(fill!(
                vec![
                    (*x - dx + px, *y - dy + py),
                    (*x + dx + px, *y + dy + py),
                    (*x + dx - px, *y + dy - py),
                    (*x - dx - px, *y - dy - py),
                ],
                color: color
            ));
        }
    }
}

fn push_bolt(draws: &mut Vec<Draw>, x: f32, y: f32, h: f32) {
    let color = Color::from_rgba(255, 214, 90, 240);
    draws.push(fill!(
        vec![
            (x + h * 0.28, y),
            (x, y + h * 0.48),
            (x + h * 0.22, y + h * 0.48),
            (x + h * 0.08, y + h),
            (x + h * 0.55, y + h * 0.38),
            (x + h * 0.32, y + h * 0.38),
        ],
        color: color
    ));
}

fn sky_draws(width: f32, height: f32, code: i64, is_day: bool) -> Vec<Draw> {
    let mut draws = Vec::new();
    let cloud = Color::from_rgba(236, 242, 248, 210);
    let cloud_dim = Color::from_rgba(198, 210, 224, 180);
    let orb_x = width * 0.88;
    let orb_y = height * 0.18;
    if !is_day {
        push_stars(&mut draws, width, height);
    }
    match sky_kind(code) {
        SkyKind::Clear => {
            push_orb(&mut draws, orb_x, orb_y, height * 0.11, is_day);
            if is_day {
                push_cloud(&mut draws, width * 0.04, height * 0.28, height * 0.14, cloud_dim);
            }
        }
        SkyKind::Partly => {
            push_orb(&mut draws, orb_x, orb_y, height * 0.10, is_day);
            push_cloud(&mut draws, width * 0.72, height * 0.08, height * 0.20, cloud);
            push_cloud(&mut draws, width * 0.04, height * 0.30, height * 0.16, cloud_dim);
        }
        SkyKind::Overcast => {
            push_cloud(&mut draws, -height * 0.06, height * 0.04, height * 0.26, cloud);
            push_cloud(&mut draws, width * 0.62, height * 0.02, height * 0.24, cloud_dim);
            push_cloud(&mut draws, width * 0.08, height * 0.32, height * 0.18, cloud_dim);
        }
        SkyKind::Fog => {
            push_orb(&mut draws, orb_x, orb_y, height * 0.07, is_day);
            for top in [0.28, 0.46, 0.64] {
                draws.push(solid_rect(
                    0.0,
                    height * top,
                    width,
                    26.0,
                    Color::from_rgba(221, 231, 248, 72),
                ));
            }
        }
        SkyKind::Rain => {
            push_cloud(&mut draws, -height * 0.04, height * 0.02, height * 0.24, cloud);
            push_cloud(&mut draws, width * 0.70, height * 0.04, height * 0.22, cloud_dim);
            push_rain(
                &mut draws,
                &[
                    (width * 0.04, height * 0.28, height * 0.18),
                    (width * 0.10, height * 0.42, height * 0.16),
                    (width * 0.16, height * 0.58, height * 0.14),
                    (width * 0.84, height * 0.32, height * 0.18),
                    (width * 0.90, height * 0.48, height * 0.16),
                    (width * 0.96, height * 0.62, height * 0.14),
                ],
            );
        }
        SkyKind::Snow => {
            push_cloud(&mut draws, -height * 0.02, height * 0.04, height * 0.22, cloud);
            push_cloud(&mut draws, width * 0.72, height * 0.06, height * 0.20, cloud_dim);
            push_flakes(
                &mut draws,
                &[
                    (width * 0.06, height * 0.34, height * 0.040),
                    (width * 0.14, height * 0.52, height * 0.034),
                    (width * 0.08, height * 0.70, height * 0.028),
                    (width * 0.88, height * 0.36, height * 0.040),
                    (width * 0.94, height * 0.56, height * 0.032),
                    (width * 0.82, height * 0.68, height * 0.028),
                ],
            );
        }
        SkyKind::Thunder => {
            push_cloud(&mut draws, -height * 0.04, height * 0.02, height * 0.26, Color::from_rgba(202, 214, 234, 210));
            push_cloud(&mut draws, width * 0.68, height * 0.04, height * 0.24, cloud_dim);
            push_bolt(&mut draws, width * 0.08, height * 0.34, height * 0.22);
            push_rain(
                &mut draws,
                &[
                    (width * 0.04, height * 0.52, height * 0.16),
                    (width * 0.12, height * 0.64, height * 0.14),
                    (width * 0.90, height * 0.56, height * 0.16),
                    (width * 0.96, height * 0.68, height * 0.12),
                ],
            );
        }
    }
    draws
}

fn saver_stat(label: &str, value: &str) -> Node {
    col(
        props!(
            gap: 6.0,
            padding: 18.0,
            background: Color::from_rgba(16, 27, 43, 150),
            border_radius: 18.0,
            border_width: 1.0,
            border_color: Color::from_rgba(255, 255, 255, 33),
        ),
        [
            text(value, style!(size: 44, weight: FontWeight::BOLD, color: Color::from_rgb(246, 250, 255), line_height: 1.0)),
            text(label, style!(size: 24, weight: FontWeight::SEMIBOLD, color: Color::from_rgb(183, 202, 227), line_height: 1.0)),
        ],
    )
}

fn hash_pill_sized(app: &App, value_size: u32, pad: f32) -> Node {
    let numbers = fleet_numbers(app);
    let (hash, zec_hash) = fleet_hash_lines(numbers.sha, numbers.zec);
    let mut rows = vec![text(hash, style!(size: value_size, weight: FontWeight::BOLD, color: Color::from_rgb(144, 219, 255), line_height: 1.0))];
    if let Some(zec) = zec_hash {
        rows.push(text(
            zec,
            style!(size: (value_size / 2).max(12), weight: FontWeight::SEMIBOLD, color: Color::from_rgb(157, 180, 208), line_height: 1.0),
        ));
    }
    col(
        props!(
            gap: 6.0,
            padding: pad,
            background: Color::from_rgba(18, 30, 48, 150),
            border_radius: 18.0,
            border_width: 1.0,
            border_color: Color::from_rgba(255, 255, 255, 31),
            cross_align: CrossAlign::Center,
        ),
        rows,
    )
}

fn hash_pill(app: &App) -> Node {
    hash_pill_sized(app, 32, 14.0)
}

fn saver_shell(width: f32, height: f32, draws: Vec<Draw>, body: Node) -> Node {
    col(
        props!(width: width, height: height, background: Color::from_rgb(3, 7, 14)),
        [
            canvas(
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                draws,
            ),
            col(
                props!(
                    width: width,
                    height: height,
                    justify_content: Justify::Center,
                    cross_align: CrossAlign::Center,
                    gap: 10.0,
                    padding: 28.0,
                ),
                [body],
            ),
            touchable(
                "saver-wake",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn screensaver_view(app: &App, width: f32, height: f32) -> Node {
    let height = screen_h(height);
    if app.saver_weather {
        let weather = &app.weather;
        let (stops, headline, sub, label) = if !app.outbound {
            (
                [(33, 94, 171), (10, 42, 94), (5, 23, 47)],
                "Weather is off".to_owned(),
                OUTBOUND_WEATHER_ERROR.to_owned(),
                "Weather".to_owned(),
            )
        } else if app.place.trim().is_empty() {
            (
                [(33, 94, 171), (10, 42, 94), (5, 23, 47)],
                "No weather location set".to_owned(),
                "Add a weather location in Settings.".to_owned(),
                "Weather".to_owned(),
            )
        } else if !weather.ready {
            let message = if weather.error.is_empty() {
                "Looking up weather".to_owned()
            } else {
                weather.error.clone()
            };
            (
                [(33, 94, 171), (10, 42, 94), (5, 23, 47)],
                message,
                app.place.clone(),
                "Weather".to_owned(),
            )
        } else {
            (
                weather_stops(weather.code, weather.is_day),
                weather_temp(weather.temp_c, app.weather_f),
                weather_description(weather.code).to_owned(),
                if weather.label.is_empty() { app.place.clone() } else { weather.label.clone() },
            )
        };
        let mut draws = gradient_bands(width, height, stops);
        if weather.ready {
            draws.extend(sky_draws(width, height, weather.code, weather.is_day));
        }
        let mut column = vec![
            text(label, style!(size: 42, weight: FontWeight::BOLD, color: Color::from_rgb(239, 246, 255), line_height: 1.0)),
            text(headline, style!(size: 112, weight: FontWeight::BOLD, color: Color::from_rgb(248, 251, 255), line_height: 1.0)),
            text(sub, style!(size: 36, weight: FontWeight::SEMIBOLD, color: Color::from_rgb(214, 230, 250), line_height: 1.0)),
        ];
        if weather.ready {
            column.push(row(
                props!(gap: 16.0, cross_align: CrossAlign::Center),
                [
                    saver_stat("Feels Like", &weather_temp(weather.feels_c, app.weather_f)),
                    saver_stat("Wind", &weather_wind(weather.wind_kmh, weather.wind_deg, app.weather_f)),
                    saver_stat("Humidity", &format!("{}%", weather.humidity)),
                    hash_pill_sized(app, 44, 18.0),
                ],
            ));
        } else {
            column.push(hash_pill_sized(app, 44, 18.0));
        }
        return saver_shell(width, height, draws, col(props!(gap: 14.0, cross_align: CrossAlign::Center), column));
    }
    let draws = gradient_bands(width, height, [(16, 27, 43), (6, 12, 22), (3, 7, 14)]);
    saver_shell(
        width,
        height,
        draws,
        col(
            props!(gap: 12.0, cross_align: CrossAlign::Center),
            [
                clock_face(),
                text(date_text(), style!(size: 44, weight: FontWeight::SEMIBOLD, color: Color::from_rgb(184, 203, 227), line_height: 1.2)),
                hash_pill(app),
            ],
        ),
    )
}

fn place_hit(index: usize, label: &str) -> Node {
    col(
        props!(
            width: 760.0,
            height: 36.0,
            background: Color::from_hex(0x1C_2E_24),
            border_radius: 8.0,
            border_width: 1.5,
            border_color: Color::from_hex(0x3E_6B_50),
            justify_content: Justify::Center,
            padding: 8.0,
        ),
        [
            text(label, style!(size: 18, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0)),
            touchable(
                &format!("hit-{index}"),
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn place_results(app: &App) -> Node {
    if app.places.is_empty() {
        let hint = if app.place.trim().chars().count() < 2 {
            "Type a city"
        } else if app.suggest_fetch.is_some() {
            "Searching"
        } else {
            "No matches"
        };
        return text(hint, style!(size: 16, color: LABEL, line_height: 1.0));
    }
    let rows = app
        .places
        .iter()
        .enumerate()
        .map(|(index, hit)| place_hit(index, &hit.label))
        .collect::<Vec<_>>();
    col(props!(width: 760.0, gap: 4.0), rows)
}

fn key_cap_w(id: &str, label: &str, width: f32) -> Node {
    col(
        props!(
            width: width,
            height: 52.0,
            background: Color::from_hex(0x1C_2E_24),
            border_radius: 8.0,
            border_width: 1.5,
            border_color: Color::from_hex(0x3E_6B_50),
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(label, style!(size: 26, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0, align: TextAlign::Center, valign: VerticalAlign::Center)),
            touchable(id, props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0), Vec::<Draw>::new()),
        ],
    )
}

fn key_row(letters: &str) -> Node {
    let keys = letters
        .chars()
        .map(|ch| key_cap_w(&format!("k-{ch}"), &ch.to_string(), 92.0))
        .collect::<Vec<_>>();
    row(props!(gap: 8.0, cross_align: CrossAlign::Center, justify_content: Justify::Center), keys)
}

fn place_keyboard(app: &App, width: f32, height: f32) -> Node {
    let naming = app.alarm_naming;
    let raw: &str = if naming { &app.alarm_name } else { &app.place };
    let shown = if raw.is_empty() {
        if naming { "Alarm name".to_owned() } else { "City name".to_owned() }
    } else {
        raw.to_owned()
    };
    let title = if naming { "Alarm name" } else { "Weather location" };
    let shown_color = if raw.is_empty() { LABEL } else { WHITE };
    with_theme(
        width,
        height,
        col(
            props!(background: BAR_FILL, width: width, height: height, gap: 0.0),
            [
                row(
                    props!(height: FLEET_HEADER_H, width: width, gap: 16.0, cross_align: CrossAlign::Center, background: BAR_FILL, padding: 8.0),
                    [
                        back_button("k-cancel"),
                        text(title, style!(size: 28, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
                    ],
                ),
                col(
                    props!(width: width, height: height - FLEET_HEADER_H, background: METRIC_FILL, gap: 6.0, justify_content: Justify::Center, cross_align: CrossAlign::Center),
                    [
                        text(shown, style!(size: 28, weight: FontWeight::BOLD, color: shown_color, line_height: 1.0)),
                        place_results(app),
                        key_row("qwertyuiop"),
                        key_row("asdfghjkl"),
                        key_row("zxcvbnm"),
                        row(
                            props!(gap: 8.0, cross_align: CrossAlign::Center),
                            [
                                key_cap_w("k-space", "Space", 220.0),
                                key_cap_w("k-back", "Delete", 180.0),
                                key_cap_w("k-done", "Done", 180.0),
                            ],
                        ),
                    ],
                ),
            ],
        ),
    )
}

fn alarm_view(app: &App, width: f32, height: f32) -> Node {
    let hour = app.alarm_hour;
    let minute = app.alarm_minute;
    let ampm = if app.alarm_pm { "PM" } else { "AM" };
    let clock = format!("{hour}:{minute:02} {ampm}");
    let sound = ALARM_SOUNDS
        .get(app.alarm_sound as usize)
        .map(|item| item.1)
        .unwrap_or(ALARM_SOUNDS[0].1);
    let name = if app.alarm_name.is_empty() { "—".to_owned() } else { app.alarm_name.clone() };
    let mut days = Vec::new();
    for (index, label) in ALARM_DAYS.iter().enumerate() {
        let on = app.alarm_days & (1 << index) != 0;
        days.push(settings_mode_pill(&format!("alarm-day-{index}"), label, on));
    }
    with_theme(
        width,
        height,
        col(
            props!(background: BAR_FILL, width: width, height: height, gap: 0.0),
            [
                row(
                    props!(height: FLEET_HEADER_H, width: width, gap: 16.0, cross_align: CrossAlign::Center, justify_content: Justify::SpaceBetween, background: BAR_FILL, padding: 8.0),
                    [
                        row(
                            props!(gap: 16.0, cross_align: CrossAlign::Center),
                            [
                                back_button("alarm-close"),
                                text("Add New Alarm", style!(size: 28, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
                            ],
                        ),
                        settings_pill("alarm-add", "Add New Alarm"),
                    ],
                ),
                col(
                    props!(width: width, height: height - FLEET_HEADER_H, background: METRIC_FILL, padding: 16.0, gap: 12.0),
                    [
                        text("Time", style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
                        row(
                            props!(gap: 8.0, cross_align: CrossAlign::Center, wrap: true),
                            [
                                settings_pill("alarm-h-down", "Hour −"),
                                settings_pill("alarm-h-up", "Hour +"),
                                text(clock, style!(size: 32, weight: FontWeight::BOLD, color: EMERALD, line_height: 1.0)),
                                settings_pill("alarm-m-down", "Min −"),
                                settings_pill("alarm-m-up", "Min +"),
                                settings_mode_pill("alarm-ampm", ampm, true),
                            ],
                        ),
                        text("Repeat", style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
                        row(props!(gap: 8.0, cross_align: CrossAlign::Center, wrap: true), days),
                        text("Alarm Sound", style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
                        row(
                            props!(gap: 8.0, cross_align: CrossAlign::Center, wrap: true),
                            [
                                settings_pill("alarm-sound-prev", "Prev"),
                                text(sound, style!(size: 22, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0)),
                                settings_pill("alarm-sound-next", "Next"),
                                settings_pill("alarm-play", "Play"),
                            ],
                        ),
                        text(
                            format!("Alarm Name  {name}"),
                            style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                        ),
                        row(
                            props!(gap: 8.0, cross_align: CrossAlign::Center),
                            [settings_pill("alarm-name", "Edit name")],
                        ),
                        text("Snooze", style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0)),
                        row(
                            props!(gap: 8.0, cross_align: CrossAlign::Center, wrap: true),
                            [
                                settings_mode_pill("alarm-snooze-on", "On", app.alarm_snooze),
                                settings_mode_pill("alarm-snooze-off", "Off", !app.alarm_snooze),
                                settings_mode_pill("alarm-limit-1", "Forever", app.alarm_snooze && app.alarm_limit == 1),
                                settings_mode_pill("alarm-limit-3", "3 times", app.alarm_snooze && app.alarm_limit == 2),
                                settings_mode_pill("alarm-limit-5", "5 times", app.alarm_snooze && app.alarm_limit == 3),
                            ],
                        ),
                    ],
                ),
            ],
        ),
    )
}

fn tailscale_status_line(state: &str) -> &'static str {
    match state {
        "connected" => "Connected",
        "needs_login" => "Sign In",
        "starting" => "Starting",
        "stopped" => "Stopped",
        "" => "Checking",
        _ => "Not running",
    }
}

fn tailscale_setup_line(app: &App) -> Option<&'static str> {
    match app.tailscale_state.as_str() {
        "needs_login" if app.tailscale_url.is_empty() => Some("Creating a new code."),
        "needs_login" => None,
        "starting" => Some("Tailscale is starting on this Deck."),
        "stopped" => Some("Tailscale is stopped. Start it to connect this Deck."),
        "connected" => None,
        _ => Some("Tailscale is not running on this Deck."),
    }
}

fn tailscale_button(id: &str, label: &str, width: f32) -> Node {
    col(
        props!(
            width: width,
            height: 64.0,
            background: Color::from_hex(0x1C_2E_24),
            border_radius: 14.0,
            border_width: 1.5,
            border_color: Color::from_hex(0x3E_6B_50),
            padding: 12.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                label,
                style!(
                    size: 26,
                    weight: FontWeight::SEMIBOLD,
                    color: WHITE,
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                ),
            ),
            touchable(
                id,
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn tailscale_actions(app: &App) -> Vec<Node> {
    match app.tailscale_state.as_str() {
        "stopped" => vec![tailscale_button("tailscale-start", "Start", 168.0)],
        "connected" => vec![
            tailscale_button("tailscale-stop", "Stop", 150.0),
            tailscale_button("tailscale-restart", "Restart", 176.0),
            tailscale_button("tailscale-reset", "Reset Tailscale", 300.0),
        ],
        _ => Vec::new(),
    }
}

fn tailscale_qr(url: &str, caption: &str, size: f32) -> Node {
    let inner = (size - 16.0).max(40.0);
    col(
        props!(gap: 10.0, cross_align: CrossAlign::Center, width: size + 80.0),
        [
            col(
                props!(
                    width: size,
                    height: size,
                    background: WHITE,
                    border_radius: 8.0,
                    padding: 8.0,
                ),
                [canvas(
                    props!(width: inner, height: inner),
                    [Draw::qr(
                        0.0,
                        0.0,
                        inner,
                        url,
                        QrStyle {
                            dark: BLACK,
                            light: WHITE,
                            quiet_zone: 2,
                        },
                    )],
                )],
            ),
            text(
                caption,
                style!(
                    size: 22,
                    weight: FontWeight::BOLD,
                    color: WHITE,
                    line_height: 1.1,
                    align: TextAlign::Center,
                ),
            ),
        ],
    )
}

fn tailscale_logo(height: f32) -> Node {
    let width = height * (632.0 / 114.0);
    canvas(
        props!(width: width, height: height),
        [Draw::bitmap(0.0, 0.0, width, height, &TAILSCALE_LOGO)],
    )
}

fn tailscale_cat() -> Node {
    let height = 280.0;
    let width = height * (834.0 / 1001.0);
    canvas(
        props!(width: width, height: height),
        [Draw::bitmap(0.0, 0.0, width, height, &TAILSCALE_CAT)],
    )
}

fn tailscale_device_name(reported: &str) -> String {
    if reported.is_empty()
        || reported.eq_ignore_ascii_case("hashwatcher-deck")
        || reported.eq_ignore_ascii_case("braiins-deck")
    {
        "HashWatcher-Deck".to_owned()
    } else {
        reported.to_owned()
    }
}

fn tailscale_step(label: &str, color: Color) -> Node {
    text(
        label,
        style!(size: 24, weight: FontWeight::BOLD, color: color, line_height: 1.0),
    )
}

fn tailscale_view(app: &App, width: f32, height: f32) -> Node {
    let host = tailscale_device_name(&app.tailscale_host);
    let connected = app.tailscale_state == "connected";
    let status_color = if connected {
        EMERALD
    } else if app.tailscale_state == "needs_login" {
        SHARE_ORANGE
    } else {
        LABEL
    };
    let subnet_pending = connected && app.tailscale_subnet != "approved";
    let show_login_qr = app.tailscale_state == "needs_login" && !app.tailscale_url.is_empty();
    let show_subnet_qr = subnet_pending && !app.tailscale_admin.is_empty();
    let mut side = Vec::new();
    let qr_size = if show_login_qr {
        side.push(tailscale_qr(&app.tailscale_url, "Scan to sign in", 320.0));
        320.0
    } else if show_subnet_qr {
        side.push(tailscale_qr(
            &app.tailscale_admin,
            "Scan to approve subnet routes",
            300.0,
        ));
        300.0
    } else {
        0.0
    };
    let show_cat = connected && qr_size == 0.0;
    if show_cat {
        side.push(tailscale_cat());
    }
    let text_w = if show_cat {
        width * 0.46
    } else if qr_size == 0.0 {
        width - 40.0
    } else {
        (width - qr_size - 180.0).max(460.0)
    };
    let mut lines = Vec::new();
    lines.push(tailscale_logo(if show_cat { 64.0 } else if subnet_pending { 64.0 } else { 80.0 }));
    lines.push(text(
        tailscale_status_line(&app.tailscale_state),
        style!(size: if show_cat { 52 } else { 56 }, weight: FontWeight::BOLD, color: status_color, line_height: 1.0),
    ));
    if !app.tailscale_ip.is_empty() {
        lines.push(text(
            "Tailscale address",
            style!(size: if show_cat { 28 } else { 32 }, weight: FontWeight::BOLD, color: LABEL, line_height: 1.0),
        ));
        lines.push(text(
            &app.tailscale_ip,
            style!(
                size: if show_cat { 56 } else if subnet_pending { 48 } else { 64 },
                weight: FontWeight::BOLD,
                color: EMERALD,
                line_height: 1.0
            ),
        ));
    }
    lines.push(text(
        host,
        style!(
            size: if show_cat { 36 } else if subnet_pending { 32 } else { 40 },
            weight: FontWeight::BOLD,
            color: WHITE,
            line_height: 1.0
        ),
    ));
    let deck = deck_url();
    if !deck.is_empty() {
        lines.push(text(
            deck,
            style!(size: if show_cat { 32 } else { 36 }, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0),
        ));
    }
    if connected {
        lines.push(tailscale_step("Signed in", EMERALD));
    }
    if subnet_pending {
        lines.push(tailscale_step("Approve subnet routes", SHARE_ORANGE));
        lines.push(text(
            "After scanning, scroll down to subnets, click Review, tap your subnet, and click Save.",
            style!(size: 22, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.15),
        ));
    }
    if connected && !app.tailscale_expiry.is_empty() {
        lines.push(tailscale_step(
            &format!("Key expires {}", app.tailscale_expiry),
            WHITE,
        ));
    }
    if let Some(hint) = tailscale_setup_line(app) {
        lines.push(text(
            hint,
            style!(size: 20, color: LABEL, line_height: 1.15),
        ));
    }
    let actions = tailscale_actions(app);
    let body_h = height - FLEET_HEADER_H;
    let text_h = (body_h - 16.0).max(40.0);
    let side_h = if show_cat && !actions.is_empty() {
        (text_h - 76.0).max(40.0)
    } else {
        text_h
    };
    let mut screen = vec![
        row(
            props!(height: FLEET_HEADER_H, width: width, gap: 16.0, cross_align: CrossAlign::Center, background: BAR_FILL, padding: 8.0),
            [
                back_button("tailscale-close"),
                text(
                    "Tailscale | Remote Monitoring",
                    style!(size: 28, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: WHITE, line_height: 1.0),
                ),
            ],
        ),
        row(
            props!(width: width, height: body_h, background: METRIC_FILL, padding: 8.0, gap: if show_cat { 24.0 } else { 72.0 }, cross_align: CrossAlign::Start),
            [
                col(
                    props!(
                        gap: if show_cat { 0.0 } else if subnet_pending { 1.0 } else { 2.0 },
                        width: text_w,
                        height: if show_cat { text_h } else { 0.0 },
                        justify_content: if show_cat { Justify::SpaceBetween } else { Justify::Start },
                    ),
                    lines,
                ),
                col(
                    props!(
                        width: (width - text_w - if show_cat { 40.0 } else { 88.0 }).max(220.0),
                        gap: 8.0,
                        height: side_h,
                        justify_content: if show_cat { Justify::Center } else { Justify::Start },
                        cross_align: CrossAlign::Center,
                    ),
                    side,
                ),
            ],
        ),
    ];
    if !actions.is_empty() {
        screen.push(row(
            props!(
                inset_right: 12.0,
                inset_bottom: 12.0,
                gap: 16.0,
                cross_align: CrossAlign::Center,
            ),
            actions,
        ));
    }
    with_theme(
        width,
        height,
        col(
            props!(background: BAR_FILL, width: width, height: height, gap: 0.0),
            screen,
        ),
    )
}

fn deck_url() -> String {
    let ip = bmc_wasm_sdk::network::info().ip;
    if ip.is_empty() {
        String::new()
    } else {
        format!("http://{ip}")
    }
}

fn settings_face(on: bool) -> NinePatch {
    ensure_nine_patch_registered(if on {
        &SETTINGS_BUTTON_ON
    } else {
        &SETTINGS_BUTTON
    })
}

fn with_settings_face(mut props: PropsData, on: bool) -> PropsData {
    props!(@set props, bg_nine_patch: settings_face(on));
    props
}

fn settings_cell(
    id: &str,
    label: &str,
    selected: Option<bool>,
    width: f32,
    height: f32,
    size: u32,
) -> Node {
    let on = selected.unwrap_or(false);
    col(
        with_settings_face(
            props!(
                width: width,
                height: height,
                padding: 4.0,
                justify_content: Justify::Center,
                cross_align: CrossAlign::Center,
            ),
            on,
        ),
        [
            text(
                label,
                style!(
                    size: size,
                    weight: FontWeight::SEMIBOLD,
                    color: if on { EMERALD } else { WHITE },
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                    text_overflow: TextOverflow::Ellipsis,
                    max_width: (width - 12.0).max(24.0) as u32,
                ),
            ),
            touchable(
                id,
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn settings_cell_width(count: usize, width: f32, gap: f32) -> f32 {
    let n = count.max(1) as f32;
    ((width - gap * (n - 1.0)) / n).max(40.0)
}

fn settings_fill(items: &[(&str, &str, Option<bool>)], width: f32, height: f32, size: u32) -> Node {
    let gap = 8.0;
    let cell_w = settings_cell_width(items.len(), width, gap);
    let mut buttons = Vec::new();
    for (id, label, selected) in items {
        buttons.push(settings_cell(id, label, *selected, cell_w, height, size));
    }
    row(
        props!(width: width, height: height, gap: gap, cross_align: CrossAlign::Center),
        buttons,
    )
}

fn settings_group(
    label: &str,
    items: &[(&str, &str, Option<bool>)],
    width: f32,
    height: f32,
    size: u32,
    max_cell: f32,
) -> Node {
    let label_w = 168.0;
    let gap = 10.0;
    let n = items.len().max(1) as f32;
    let available = (width - label_w - 12.0).max(80.0);
    let natural = (available - gap * (n - 1.0)) / n;
    let cell_w = natural.min(max_cell).max(64.0);
    let mut buttons = Vec::new();
    for (id, name, selected) in items {
        buttons.push(settings_cell(id, name, *selected, cell_w, height, size));
    }
    row(
        props!(width: width, height: height, gap: 12.0, cross_align: CrossAlign::Center),
        [
            col(
                props!(width: label_w, height: height, justify_content: Justify::Center),
                [text(
                    label,
                    style!(
                        size: 22,
                        weight: FontWeight::BOLD,
                        color: WHITE,
                        line_height: 1.0,
                        text_overflow: TextOverflow::Ellipsis,
                        max_width: label_w as u32,
                    ),
                )],
            ),
            row(
                props!(gap: gap, cross_align: CrossAlign::Center),
                buttons,
            ),
        ],
    )
}

fn settings_bar(id: &str, title: &str, detail: &str, detail_color: Color, width: f32, height: f32) -> Node {
    let inner = (width - 32.0).max(40.0);
    let title_max = if detail.is_empty() { inner } else { (inner * 0.62).max(120.0) };
    let mut kids = vec![text(
        title,
        style!(
            size: 26,
            weight: FontWeight::BOLD,
            color: WHITE,
            line_height: 1.0,
            text_overflow: TextOverflow::Ellipsis,
            max_width: title_max as u32,
        ),
    )];
    if !detail.is_empty() {
        kids.push(text(
            detail,
            style!(
                size: 22,
                weight: FontWeight::SEMIBOLD,
                color: detail_color,
                line_height: 1.0,
                text_overflow: TextOverflow::Ellipsis,
                max_width: (inner - title_max).max(80.0) as u32,
            ),
        ));
    }
    kids.push(touchable(
        id,
        props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
        Vec::<Draw>::new(),
    ));
    row(
        with_settings_face(
            props!(
                width: width,
                height: height,
                padding: 16.0,
                justify_content: if detail.is_empty() { Justify::Start } else { Justify::SpaceBetween },
                cross_align: CrossAlign::Center,
            ),
            false,
        ),
        kids,
    )
}

fn tailscale_status_color(state: &str) -> Color {
    match state {
        "connected" => EMERALD,
        "needs_login" => SHARE_ORANGE,
        _ => LABEL,
    }
}

fn settings_box(width: f32, height: f32) -> (f32, f32) {
    let height = screen_h(height);
    let body_h = (height - FLEET_HEADER_H).max(120.0);
    let pad = 12.0;
    (width - pad * 2.0, body_h - pad * 2.0)
}

fn settings_frame(title: &str, back_id: &str, width: f32, height: f32, body: Node) -> Node {
    let height = screen_h(height);
    let body_h = (height - FLEET_HEADER_H).max(120.0);
    let pad = 12.0;
    with_theme(
        width,
        height,
        col(
            props!(background: BAR_FILL, width: width, height: height, gap: 0.0),
            [
                row(
                    props!(
                        height: FLEET_HEADER_H,
                        width: width,
                        gap: 16.0,
                        cross_align: CrossAlign::Center,
                        justify_content: Justify::SpaceBetween,
                        background: BAR_FILL,
                        padding: 6.0,
                    ),
                    [
                        row(
                            props!(gap: 16.0, cross_align: CrossAlign::Center),
                            [
                                back_button(back_id),
                                text(
                                    title,
                                    style!(size: 28, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: WHITE, line_height: 1.0),
                                ),
                                text(
                                    format!("v{DECK_VERSION}"),
                                    style!(size: 18, weight: FontWeight::SEMIBOLD, color: EMERALD, line_height: 1.0),
                                ),
                            ],
                        ),
                        text(
                            deck_url(),
                            style!(size: 24, weight: FontWeight::SEMIBOLD, color: EMERALD, line_height: 1.0),
                        ),
                    ],
                ),
                row(
                    props!(width: width, height: body_h, background: METRIC_FILL, padding: pad),
                    [body],
                ),
            ],
        ),
    )
}

fn settings_menu(app: &App, width: f32, height: f32) -> Node {
    let (view_w, view_h) = settings_box(width, height);
    let gap = 12.0;
    let bar_h = ((view_h - gap * 3.0) / 4.0).max(52.0);
    let col_w = (view_w - gap) / 2.0;
    settings_frame(
        "Settings",
        "close-settings",
        width,
        height,
        col(
            props!(width: view_w, height: view_h, gap: gap),
            [
                row(
                    props!(width: view_w, height: bar_h, gap: gap, cross_align: CrossAlign::Center),
                    [
                        settings_bar(
                            "tailscale-open",
                            "Tailscale | Remote Monitoring",
                            "",
                            EMERALD,
                            col_w,
                            bar_h,
                        ),
                        settings_bar("settings-display", "Display | Weather", "", EMERALD, col_w, bar_h),
                    ],
                ),
                row(
                    props!(width: view_w, height: bar_h, gap: gap, cross_align: CrossAlign::Center),
                    [
                        settings_bar("settings-rgb", "RGB | Modes", "", EMERALD, col_w, bar_h),
                        settings_bar("settings-layout", "Layout | Names", "", EMERALD, col_w, bar_h),
                    ],
                ),
                row(
                    props!(width: view_w, height: bar_h, gap: gap, cross_align: CrossAlign::Center),
                    [
                        settings_bar("settings-alerts", "Alerts | Sound", "", EMERALD, col_w, bar_h),
                        settings_bar("settings-refresh", "Refresh | Poll", "", EMERALD, col_w, bar_h),
                    ],
                ),
                settings_bar("settings-support", "Support | Download", "", LABEL, view_w, bar_h),
            ],
        ),
    )
}

fn settings_display_page(app: &App, width: f32, height: f32) -> Node {
    let (view_w, _) = settings_box(width, height);
    let pct = shown_brightness(app);
    let pct_label = if pct < 10 { "—".to_owned() } else { format!("{pct}%") };
    let city = if app.place.is_empty() { "Set city".to_owned() } else { app.place.clone() };
    let slider_w = (view_w - 280.0).max(160.0);
    settings_frame(
        "Display | Weather",
        "settings-menu",
        width,
        height,
        col(
            props!(width: view_w, gap: 10.0),
            [
                row(
                    props!(width: view_w, height: 36.0, gap: 16.0, cross_align: CrossAlign::Center),
                    [
                        text(
                            "Brightness",
                            style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                        ),
                        text(
                            pct_label,
                            style!(size: 22, weight: FontWeight::BOLD, color: EMERALD, line_height: 1.0),
                        ),
                        col(
                            props!(width: slider_w, height: 36.0, justify_content: Justify::Center),
                            [progress_bar!(
                                ProgressMode::Slider(brightness_frac(pct)),
                                touch_key: "brightness",
                                track_h: 10.0,
                                fill_color: EMERALD,
                                track_color: Color::from_rgb(48, 56, 52),
                                bg_color: Color::from_rgb(22, 24, 28),
                            )],
                        ),
                    ],
                ),
                text(
                    screen_off_caption(app),
                    style!(
                        size: 16,
                        color: LABEL,
                        line_height: 1.0,
                        text_overflow: TextOverflow::Ellipsis,
                        max_width: view_w.max(40.0) as u32,
                    ),
                ),
                text(
                    "Screen saver",
                    style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                ),
                settings_fill(
                    &[
                        ("saver-0", "Off", Some(app.saver_ms == 0)),
                        ("saver-30000", "30s", Some(app.saver_ms == 30_000)),
                        ("saver-60000", "1m", Some(app.saver_ms == 60_000)),
                        ("saver-120000", "2m", Some(app.saver_ms == 120_000)),
                        ("saver-300000", "5m", Some(app.saver_ms == 300_000)),
                        ("saver-600000", "10m", Some(app.saver_ms == 600_000)),
                        ("saver-1800000", "30m", Some(app.saver_ms == 1_800_000)),
                    ],
                    view_w,
                    64.0,
                    20,
                ),
                settings_fill(
                    &[
                        ("saver-clock", "Clock", Some(!app.saver_weather)),
                        ("saver-weather", "Weather", Some(app.saver_weather)),
                        ("temp-f", "°F", Some(app.weather_f)),
                        ("temp-c", "°C", Some(!app.weather_f)),
                        ("place-edit", &city, None),
                    ],
                    view_w,
                    64.0,
                    20,
                ),
                row(
                    props!(width: view_w, height: 56.0, gap: 12.0, cross_align: CrossAlign::Center),
                    [
                        col(
                            props!(width: 230.0, height: 56.0, justify_content: Justify::Center),
                            [text(
                                "Outbound Data",
                                style!(
                                    size: 22,
                                    weight: FontWeight::BOLD,
                                    color: WHITE,
                                    line_height: 1.0,
                                ),
                            )],
                        ),
                        settings_cell("outbound-on", "On", Some(app.outbound), 160.0, 56.0, 22),
                        settings_cell("outbound-off", "Off", Some(!app.outbound), 160.0, 56.0, 22),
                    ],
                ),
                text(
                    "Used only for weather data.",
                    style!(size: 16, color: LABEL, line_height: 1.1),
                ),
            ],
        ),
    )
}

fn settings_rgb_page(app: &App, width: f32, height: f32) -> Node {
    let (view_w, _) = settings_box(width, height);
    settings_frame(
        "RGB | Modes",
        "settings-menu",
        width,
        height,
        col(
            props!(width: view_w, gap: 12.0),
            [
                text(
                    "Flash on Share Increments",
                    style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                ),
                settings_fill(
                    &[
                        ("pulse-on", "On", Some(app.share_pulse)),
                        ("pulse-off", "Off", Some(!app.share_pulse)),
                        ("pulse-test", "Test pulse", None),
                    ],
                    view_w,
                    56.0,
                    22,
                ),
                settings_led_block(app, view_w),
                text(
                    "Orange blinks when a share increases.",
                    style!(size: 16, color: LABEL, line_height: 1.1),
                ),
            ],
        ),
    )
}

fn settings_layout_page(app: &App, width: f32, height: f32) -> Node {
    let (view_w, view_h) = settings_box(width, height);
    let summary_h = (view_h - 56.0 * 2.0 - 10.0 * 2.0).max(140.0);
    settings_frame(
        "Layout | Names",
        "settings-menu",
        width,
        height,
        col(
            props!(width: view_w, height: view_h, gap: 10.0),
            [
                settings_group(
                    "Fleet layout",
                    &[
                        ("fleet-hash", "Hashrate", Some(!app.fleet_groups)),
                        ("fleet-groups", "Groups", Some(app.fleet_groups)),
                    ],
                    view_w,
                    56.0,
                    22,
                    280.0,
                ),
                settings_group(
                    "Neural names",
                    &[
                        ("names-on", "Show", Some(app.label_mode == 0)),
                        ("names-hash", "Hashrate", Some(app.label_mode == 1)),
                        ("names-off", "Hide", Some(app.label_mode == 2)),
                    ],
                    view_w,
                    56.0,
                    22,
                    240.0,
                ),
                summary_column_settings(app, view_w, summary_h),
            ],
        ),
    )
}

fn settings_alerts_page(app: &App, width: f32, height: f32) -> Node {
    let (view_w, _) = settings_box(width, height);
    settings_frame(
        "Alerts | Sound",
        "settings-menu",
        width,
        height,
        col(
            props!(width: view_w, gap: 12.0),
            [
                settings_bar("alarm-open", "Add New Alarm", "", LABEL, view_w, 64.0),
                settings_group(
                    "Celebrations",
                    &[
                        ("preview-best", "Best diff", None),
                        ("preview-block", "Block found", None),
                    ],
                    view_w,
                    56.0,
                    22,
                    280.0,
                ),
                settings_group(
                    "Share sound",
                    &[
                        ("vol-0", "0%", Some(app.sound_volume == 0)),
                        ("vol-25", "25%", Some(app.sound_volume == 25)),
                        ("vol-50", "50%", Some(app.sound_volume == 50)),
                        ("vol-75", "75%", Some(app.sound_volume == 75)),
                        ("vol-100", "100%", Some(app.sound_volume == 100)),
                        ("sound-play", "Play", None),
                    ],
                    view_w,
                    56.0,
                    22,
                    180.0,
                ),
                text(
                    "Logins come from HashWatcher.",
                    style!(size: 16, color: LABEL, line_height: 1.1),
                ),
            ],
        ),
    )
}

fn settings_refresh_page(app: &App, width: f32, height: f32) -> Node {
    let (view_w, _) = settings_box(width, height);
    let seconds = app.poll_ms / 1_000;
    settings_frame(
        "Refresh | Poll",
        "settings-menu",
        width,
        height,
        col(
            props!(width: view_w, gap: 12.0),
            [
                settings_group(
                    "Refresh",
                    &[
                        ("rate-1", "1s", Some(seconds == 1)),
                        ("rate-5", "5s", Some(seconds == 5)),
                        ("rate-8", "8s", Some(seconds == 8)),
                        ("rate-15", "15s", Some(seconds == 15)),
                        ("rate-30", "30s", Some(seconds == 30)),
                        ("rate-60", "60s", Some(seconds == 60)),
                        ("poll-now", "Poll now", None),
                    ],
                    view_w,
                    64.0,
                    22,
                    180.0,
                ),
            ],
        ),
    )
}

fn settings_support_page(width: f32, height: f32) -> Node {
    let (view_w, view_h) = settings_box(width, height);
    settings_frame(
        "Support | Download",
        "settings-menu",
        width,
        height,
        col(
            props!(
                width: view_w,
                height: view_h,
                gap: 8.0,
                justify_content: Justify::Center,
            ),
            [
                row(
                    props!(gap: 48.0, cross_align: CrossAlign::Center),
                    [
                        row(
                            props!(gap: 16.0, cross_align: CrossAlign::Center),
                            [
                                qr_tile(112.0, &DOWNLOAD_QR),
                                col(
                                    props!(gap: 4.0),
                                    [
                                        text(
                                            "Download HashWatcher",
                                            style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                                        ),
                                        text(
                                            "www.hashwatcher.app",
                                            style!(size: 18, weight: FontWeight::SEMIBOLD, color: EMERALD, line_height: 1.0),
                                        ),
                                    ],
                                ),
                            ],
                        ),
                        row(
                            props!(gap: 16.0, cross_align: CrossAlign::Center),
                            [
                                qr_tile(112.0, &SUPPORT_QR),
                                col(
                                    props!(gap: 4.0),
                                    [
                                        text(
                                            "Braiins Deck Support",
                                            style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                                        ),
                                        text(
                                            format!("Version {DECK_VERSION}"),
                                            style!(size: 18, weight: FontWeight::SEMIBOLD, color: EMERALD, line_height: 1.0),
                                        ),
                                    ],
                                ),
                            ],
                        ),
                    ],
                ),
            ],
        ),
    )
}

fn settings_view(app: &App, width: f32, height: f32) -> Node {
    let height = screen_h(height);
    if app.place_edit || app.alarm_naming {
        return place_keyboard(app, width, height);
    }
    if app.alarm_open {
        return alarm_view(app, width, height);
    }
    if app.tailscale_open {
        return tailscale_view(app, width, height);
    }
    match app.settings_page {
        1 => settings_display_page(app, width, height),
        2 => settings_rgb_page(app, width, height),
        3 => settings_layout_page(app, width, height),
        4 => settings_alerts_page(app, width, height),
        5 => settings_refresh_page(app, width, height),
        6 => settings_support_page(width, height),
        _ => settings_menu(app, width, height),
    }
}

fn summary_name_row(app: &App, cols: &[u8], width: f32, height: f32) -> Node {
    let gap = 8.0;
    let cell_w = settings_cell_width(cols.len(), width, gap);
    let mut buttons = Vec::new();
    for col_id in cols {
        let id = format!("colopt-{}", summary_col_id(*col_id));
        buttons.push(settings_cell(
            &id,
            summary_col_title(*col_id),
            Some(app.summary_cols.contains(col_id)),
            cell_w,
            height,
            18,
        ));
    }
    row(
        props!(width: width, height: height, gap: gap, cross_align: CrossAlign::Center),
        buttons,
    )
}

fn summary_column_settings(app: &App, width: f32, height: f32) -> Node {
    let gap = 6.0;
    let title_h = 26.0;
    let rest = (height - title_h - gap * 3.0).max(90.0);
    let slot_h = (rest * 0.36).clamp(46.0, 60.0);
    let names_h = (rest - slot_h) / 2.0;
    let slot_gap = 8.0;
    let slot_w = settings_cell_width(app.summary_cols.len(), width, slot_gap);
    let mut slots = Vec::new();
    for (index, col_id) in app.summary_cols.iter().enumerate() {
        let id = format!("colslot-{index}");
        slots.push(settings_cell(
            &id,
            summary_col_long(*col_id),
            Some(app.summary_edit as usize == index),
            slot_w,
            slot_h,
            18,
        ));
    }
    col(
        props!(width: width, height: height, gap: gap, justify_content: Justify::Start),
        [
            col(
                props!(width: width, height: title_h, justify_content: Justify::Center),
                [text(
                    "Detailed summary",
                    style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                )],
            ),
            row(
                props!(width: width, height: slot_h, gap: slot_gap, cross_align: CrossAlign::Center),
                slots,
            ),
            summary_name_row(app, &SUMMARY_PICK[..8], width, names_h),
            summary_name_row(app, &SUMMARY_PICK[8..], width, names_h),
        ],
    )
}

fn settings_led_block(app: &App, width: f32) -> Node {
    col(
        props!(width: width, gap: 8.0),
        [
            row(
                props!(
                    width: width,
                    height: 28.0,
                    justify_content: Justify::SpaceBetween,
                    cross_align: CrossAlign::Center,
                ),
                [
                    text(
                        "Deck LEDs",
                        style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                    ),
                    text(
                        app.led_label.clone(),
                        style!(
                            size: 18,
                            weight: FontWeight::SEMIBOLD,
                            color: LABEL,
                            line_height: 1.0,
                            text_overflow: TextOverflow::Ellipsis,
                            max_width: 480,
                        ),
                    ),
                ],
            ),
            settings_fill(
                &[
                    ("led-red", "Red", Some(led_color_on(app, (255, 32, 32)))),
                    ("led-amber", "Amber", Some(led_color_on(app, (255, 196, 0)))),
                    ("led-emerald", "Green", Some(led_color_on(app, (0, 204, 102)))),
                    ("led-blue", "Blue", Some(led_color_on(app, (0, 90, 255)))),
                    ("led-purple", "Purple", Some(led_color_on(app, (180, 40, 255)))),
                    ("led-white", "White", Some(led_color_on(app, (255, 255, 255)))),
                    ("led-off", "Off", Some(!app.led_hold)),
                ],
                width,
                58.0,
                20,
            ),
            settings_fill(
                &[
                    ("led-solid", "Solid", Some(app.led_hold && !app.led_rainbow && app.led_effect == LedEffect::Solid)),
                    ("led-breathe", "Breathe", Some(!app.led_rainbow && app.led_effect == LedEffect::Breathe)),
                    ("led-chase", "Chase", Some(!app.led_rainbow && app.led_effect == LedEffect::Chase)),
                    ("led-scan", "Scan", Some(!app.led_rainbow && app.led_effect == LedEffect::Scan)),
                    ("led-snake", "Snake", Some(!app.led_rainbow && app.led_effect == LedEffect::Snake)),
                    ("led-rider", "Night Rider", Some(!app.led_rainbow && app.led_effect == LedEffect::KnightRider)),
                    ("led-rainbow", "Rainbow", Some(app.led_rainbow)),
                ],
                width,
                58.0,
                20,
            ),
        ],
    )
}

fn header_figure(value: &str, color: Color, size: u32) -> Node {
    text(
        value,
        style!(
            size: size,
            weight: FontWeight::SEMIBOLD,
            color: color,
            line_height: 1.0,
            align: TextAlign::Center,
            valign: VerticalAlign::Center,
        ),
    )
}

fn neural_clock_group(app: &App, hash: &str, zec: Option<&str>, shares: &str) -> Vec<Node> {
    let clock = header_figure(&clock_text(), WHITE, 36);
    if !app.neural {
        return vec![clock];
    }
    let share_color = if app.share_flash_ms > 0 { SHARE_ORANGE } else { WHITE };
    let mut items = vec![header_figure(hash, EMERALD, 22)];
    if let Some(zec) = zec {
        items.push(header_figure(zec, LABEL, 18));
    }
    items.push(header_figure(shares, share_color, 22));
    items.push(clock);
    items
}

fn best_popup(popup: &BestPopup, width: f32, height: f32) -> Node {
    if popup.kind == 1 {
        return block_popup(popup, width, height);
    }
    let height = screen_h(height);
    let image_h = (height - 36.0).min(420.0);
    let image_w = image_h * (1708.0 / 921.0);
    let text_w = (width - image_w - 56.0).max(280.0);
    let bitmap = if popup.graphic == 0 { &BEST_DIFF_IMG } else { &BEST_DIFF_ALT };
    let (number, unit) = split_difficulty(&format_difficulty(Some(popup.difficulty)));
    let mut copy = vec![
        text(
            "HASHWATCHER",
            style!(
                size: 28,
                weight: FontWeight::BOLD,
                color: POSTER_GOLD,
                line_height: 1.0,
                align: TextAlign::Center,
            ),
        ),
        text(
            "BEST DIFFICULTY",
            style!(
                size: 14,
                weight: FontWeight::BOLD,
                color: POSTER_MUTED,
                line_height: 1.0,
                align: TextAlign::Center,
            ),
        ),
        row(
            props!(gap: 8.0, justify_content: Justify::Center, cross_align: CrossAlign::End),
            [
                text(
                    number,
                    style!(
                        size: 64,
                        weight: FontWeight::BOLD,
                        color: POSTER_IVORY,
                        line_height: 1.0,
                    ),
                ),
                text(
                    unit,
                    style!(
                        size: 32,
                        weight: FontWeight::BOLD,
                        color: POSTER_GOLD,
                        line_height: 1.0,
                    ),
                ),
            ],
        ),
        text(
            popup.name.clone(),
            style!(
                size: 22,
                weight: FontWeight::SEMIBOLD,
                color: WHITE,
                line_height: 1.0,
                align: TextAlign::Center,
                text_overflow: TextOverflow::Ellipsis,
                max_width: text_w as u32,
            ),
        ),
        text(
            "just set a new personal best",
            style!(
                size: 16,
                weight: FontWeight::SEMIBOLD,
                color: POSTER_MUTED,
                line_height: 1.0,
                align: TextAlign::Center,
            ),
        ),
    ];
    let pool = popup.pool.trim();
    if !pool.is_empty() {
        copy.push(text(
            pool.to_owned(),
            style!(
                size: 18,
                weight: FontWeight::SEMIBOLD,
                color: POSTER_IVORY,
                line_height: 1.0,
                align: TextAlign::Center,
                text_overflow: TextOverflow::Ellipsis,
                max_width: text_w as u32,
            ),
        ));
    }
    copy.push(col(
        props!(
            width: text_w.min(420.0),
            height: 52.0,
            background: POSTER_GOLD,
            border_radius: 16.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                "AWESOME",
                style!(
                    size: 18,
                    weight: FontWeight::BOLD,
                    color: Color::from_rgb(26, 10, 0),
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                ),
            ),
            touchable(
                "best-awesome",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    ));
    row(
        props!(
            width: width,
            height: height,
            background: POSTER_BG,
            padding: 18.0,
            gap: 20.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            canvas(
                props!(width: image_w, height: image_h),
                [Draw::bitmap(0.0, 0.0, image_w, image_h, bitmap)],
            ),
            col(
                props!(
                    width: text_w,
                    gap: 8.0,
                    justify_content: Justify::Center,
                    cross_align: CrossAlign::Center,
                ),
                copy,
            ),
        ],
    )
}

fn block_popup(popup: &BestPopup, width: f32, height: f32) -> Node {
    let height = screen_h(height);
    let image_h = (height - 36.0).min(400.0);
    let image_w = image_h * (1667.0 / 896.0);
    let text_w = (width - image_w - 56.0).max(280.0);
    let pool = popup.pool.trim();
    let mut copy = vec![
        text(
            "HASHWATCHER",
            style!(
                size: 28,
                weight: FontWeight::BOLD,
                color: POSTER_GOLD,
                line_height: 1.0,
                align: TextAlign::Center,
            ),
        ),
        text(
            "BLOCK FOUND",
            style!(
                size: 40,
                weight: FontWeight::BOLD,
                color: POSTER_IVORY,
                line_height: 1.0,
                align: TextAlign::Center,
            ),
        ),
        text(
            popup.name.clone(),
            style!(
                size: 22,
                weight: FontWeight::SEMIBOLD,
                color: WHITE,
                line_height: 1.0,
                align: TextAlign::Center,
                text_overflow: TextOverflow::Ellipsis,
                max_width: text_w as u32,
            ),
        ),
        text(
            "just found a Bitcoin block",
            style!(
                size: 16,
                weight: FontWeight::SEMIBOLD,
                color: POSTER_MUTED,
                line_height: 1.0,
                align: TextAlign::Center,
            ),
        ),
    ];
    if !pool.is_empty() {
        copy.push(text(
            pool.to_owned(),
            style!(
                size: 18,
                weight: FontWeight::SEMIBOLD,
                color: POSTER_IVORY,
                line_height: 1.0,
                align: TextAlign::Center,
                text_overflow: TextOverflow::Ellipsis,
                max_width: text_w as u32,
            ),
        ));
    }
    copy.push(text(
        "Check your pool's interface to learn more",
        style!(
            size: 15,
            weight: FontWeight::SEMIBOLD,
            color: POSTER_GOLD,
            line_height: 1.0,
            align: TextAlign::Center,
        ),
    ));
    copy.push(col(
        props!(
            width: text_w.min(420.0),
            height: 52.0,
            background: POSTER_GOLD,
            border_radius: 16.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                "AWESOME",
                style!(
                    size: 18,
                    weight: FontWeight::BOLD,
                    color: Color::from_rgb(26, 10, 0),
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                ),
            ),
            touchable(
                "best-awesome",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    ));
    row(
        props!(
            width: width,
            height: height,
            background: Color::from_rgb(12, 5, 1),
            padding: 18.0,
            gap: 20.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            canvas(
                props!(width: image_w, height: image_h),
                [Draw::bitmap(0.0, 0.0, image_w, image_h, &BLOCK_FOUND_IMG)],
            ),
            col(
                props!(
                    width: text_w,
                    gap: 8.0,
                    justify_content: Justify::Center,
                    cross_align: CrossAlign::Center,
                ),
                copy,
            ),
        ],
    )
}

fn split_difficulty(formatted: &str) -> (String, String) {
    let unit = formatted
        .chars()
        .last()
        .filter(|mark| matches!(mark, 'K' | 'M' | 'G' | 'T' | 'P' | 'E'))
        .map(|mark| mark.to_string())
        .unwrap_or_default();
    if unit.is_empty() {
        (formatted.to_owned(), String::new())
    } else {
        (formatted[..formatted.len() - 1].to_owned(), unit)
    }
}

const SUMMARY_COL_COUNT: u8 = 16;
/// Settings picker order matches the iOS detailed-summary columns.
const SUMMARY_PICK: [u8; 16] = [0, 1, 4, 3, 5, 6, 7, 8, 2, 9, 10, 13, 11, 12, 14, 15];

fn summary_col_id(col: u8) -> &'static str {
    match col % SUMMARY_COL_COUNT {
        0 => "pool",
        1 => "hash",
        2 => "asic",
        3 => "temp",
        4 => "power",
        5 => "fan",
        6 => "eff",
        7 => "shares",
        8 => "rej",
        9 => "best",
        10 => "sess",
        11 => "freq",
        12 => "up",
        13 => "bs",
        14 => "wifi",
        _ => "status",
    }
}

fn summary_col_title(col: u8) -> &'static str {
    match col % SUMMARY_COL_COUNT {
        0 => "Pool",
        1 => "Hash",
        2 => "ASIC",
        3 => "Temp",
        4 => "Power",
        5 => "Fan",
        6 => "Eff",
        7 => "Shares",
        8 => "Rej",
        9 => "Best",
        10 => "Sess",
        11 => "Freq",
        12 => "Up",
        13 => "B/S",
        14 => "Wi-Fi",
        _ => "Status",
    }
}

fn summary_col_long(col: u8) -> &'static str {
    match col % SUMMARY_COL_COUNT {
        0 => "Pool",
        1 => "Hashrate",
        2 => "ASIC Error",
        3 => "Temperature",
        4 => "Power",
        5 => "Fan Speed",
        6 => "Efficiency",
        7 => "Shares",
        8 => "Reject Rate",
        9 => "Best Difficulty",
        10 => "Session Difficulty",
        11 => "Frequency",
        12 => "Uptime",
        13 => "Best/Session",
        14 => "Wi-Fi",
        _ => "Status",
    }
}

fn summary_col_from_id(id: &str) -> Option<u8> {
    (0..SUMMARY_COL_COUNT).find(|col| summary_col_id(*col) == id)
}

fn parse_summary_cols(text: &str) -> Vec<u8> {
    let mut cols = Vec::new();
    for part in text.split(',') {
        let Some(col) = summary_col_from_id(part.trim()) else {
            continue;
        };
        if !cols.contains(&col) {
            cols.push(col);
        }
    }
    cols
}

fn short_pool(pool: &str) -> String {
    let trimmed = pool
        .trim()
        .trim_start_matches("stratum+tcp://")
        .trim_start_matches("stratum+ssl://")
        .trim_start_matches("stratum://")
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = trimmed.split('/').next().unwrap_or(trimmed);
    let host = host.split('@').last().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host).trim();
    if host.is_empty() {
        return "—".to_owned();
    }
    let chars: Vec<char> = host.chars().collect();
    if chars.len() <= 14 {
        return host.to_owned();
    }
    let head: String = chars.iter().take(3).collect();
    let tail: String = chars.iter().skip(chars.len() - 3).collect();
    format!("{head}.....{tail}")
}

fn summary_hash_text(miner: &Miner) -> String {
    let Some(rate) = miner.hashrate_ths.filter(|value| *value > 0.0) else {
        return "—".to_owned();
    };
    if miner.family == "bitmainZEC" {
        format!("{rate:.1} KSol")
    } else {
        format_hashrate(Some(rate)).replace("/s", "")
    }
}

fn reject_text(miner: &Miner) -> String {
    let Some(rejected) = miner.shares_rejected else {
        return "—".to_owned();
    };
    let total = miner.shares_accepted.unwrap_or(0).saturating_add(rejected);
    if total == 0 {
        return "0%".to_owned();
    }
    let pct = rejected as f64 / total as f64 * 100.0;
    if pct == 0.0 {
        "0%".to_owned()
    } else if pct < 1.0 {
        format!("{pct:.2}%")
    } else if pct < 10.0 {
        format!("{pct:.1}%")
    } else {
        format!("{pct:.0}%")
    }
}

fn summary_metric(miner: &Miner, col: u8) -> (String, Color) {
    match col % SUMMARY_COL_COUNT {
        0 => (
            short_pool(&miner.pool),
            if miner.family == "bitmainZEC" { SHARE_ORANGE } else { EMERALD },
        ),
        1 => (summary_hash_text(miner), EMERALD),
        2 => match miner.asic_pct {
            Some(pct) => (format_asic(pct), if pct >= 1.0 { AMBER } else { EMERALD }),
            None => ("—".to_owned(), MUTED),
        },
        3 => (format_temp(miner.temp_c), temp_color(miner.temp_c, miner.family)),
        4 => (format_power(miner.power_w), WHITE),
        5 => (format_fan(miner.fan.or(miner.fan_rpm)), WHITE),
        6 => {
            let eff = match (miner.power_w, miner.hashrate_ths) {
                (Some(power), Some(hash)) if miner.family != "bitmainZEC" && hash > 0.0 => Some(power / hash),
                _ => None,
            };
            (format_efficiency(eff), WHITE)
        }
        7 => (count_text(miner.shares_accepted), WHITE),
        8 => (reject_text(miner), rate_color(miner)),
        9 => (format_difficulty(miner.best_diff), AMBER),
        10 => (format_difficulty(miner.best_session), AMBER),
        11 => match miner.frequency.filter(|value| *value > 0.0) {
            Some(mhz) => (format!("{mhz:.0}"), WHITE),
            None => ("—".to_owned(), MUTED),
        },
        12 => (format_uptime(miner.uptime_s), WHITE),
        13 => (
            format!(
                "{} / {}",
                format_difficulty(miner.best_diff),
                format_difficulty(miner.best_session)
            ),
            AMBER,
        ),
        15 => (status_label(miner).to_owned(), if miner.reachable { EMERALD } else { RED }),
        _ => ("—".to_owned(), MUTED),
    }
}

fn rate_color(miner: &Miner) -> Color {
    let Some(rejected) = miner.shares_rejected else {
        return MUTED;
    };
    let total = miner.shares_accepted.unwrap_or(0).saturating_add(rejected);
    if total == 0 {
        return EMERALD;
    }
    let pct = rejected as f64 / total as f64 * 100.0;
    if pct <= 0.2 {
        EMERALD
    } else if pct < 1.0 {
        AMBER
    } else {
        RED
    }
}

fn summary_totals_metric(app: &App, col: u8) -> (String, Color) {
    let numbers = fleet_numbers(app);
    let live = app
        .miners
        .iter()
        .filter(|miner| miner.reachable && miner.hashrate_ths.unwrap_or(0.0) > 0.0)
        .count();
    match col % SUMMARY_COL_COUNT {
        0 => (format!("{live} of {} live", app.miners.len()), EMERALD),
        1 => {
            let (hash, _) = fleet_hash_lines(numbers.sha, numbers.zec);
            (hash.replace("/s", ""), EMERALD)
        }
        2 => {
            let values: Vec<f64> = app.miners.iter().filter_map(|miner| miner.asic_pct).collect();
            if values.is_empty() {
                ("—".to_owned(), MUTED)
            } else {
                let avg = values.iter().sum::<f64>() / values.len() as f64;
                (format_asic(avg), if avg >= 1.0 { AMBER } else { EMERALD })
            }
        }
        3 => (numbers.temp, WHITE),
        4 => (numbers.power, WHITE),
        6 => (numbers.eff, WHITE),
        7 => (numbers.shares, WHITE),
        12 => (numbers.online, WHITE),
        15 => (numbers.online, WHITE),
        _ => ("—".to_owned(), MUTED),
    }
}

struct SummaryLayout {
    row: f32,
    name: f32,
    pool: f32,
    metric: f32,
}

fn summary_layout(width: f32, cols: &[u8; 4]) -> SummaryLayout {
    let name = 210.0;
    let metric = 156.0;
    let dot = 18.0;
    let gaps = 8.0 * 5.0;
    let pool_slots = cols.iter().filter(|col| **col % SUMMARY_COL_COUNT == 0).count() as f32;
    let fixed_slots = 4.0 - pool_slots;
    let pool = if pool_slots > 0.0 {
        ((width - dot - name - metric * fixed_slots - gaps) / pool_slots).max(120.0)
    } else {
        metric
    };
    SummaryLayout {
        row: width,
        name,
        pool,
        metric,
    }
}

fn summary_slot_width(col: u8, layout: &SummaryLayout) -> f32 {
    if col % SUMMARY_COL_COUNT == 0 {
        layout.pool
    } else {
        layout.metric
    }
}

fn format_asic(pct: f64) -> String {
    if pct <= 0.0 {
        "0%".to_owned()
    } else if pct < 1.0 {
        format!("{pct:.2}%")
    } else if pct < 10.0 {
        format!("{pct:.1}%")
    } else {
        format!("{pct:.0}%")
    }
}

fn summary_table_cell(value: &str, color: Color, width: f32, leading: bool, bold: bool) -> Node {
    col(
        props!(
            width: width,
            height: 48.0,
            justify_content: Justify::Center,
            cross_align: if leading { CrossAlign::Start } else { CrossAlign::End },
        ),
        [text(
            value,
            style!(
                size: 30,
                weight: if bold { FontWeight::BOLD } else { FontWeight::SEMIBOLD },
                color: color,
                line_height: 1.0,
                align: if leading { TextAlign::Left } else { TextAlign::Right },
                valign: VerticalAlign::Center,
                text_overflow: TextOverflow::Ellipsis,
                max_width: width.max(20.0) as u32,
            ),
        )],
    )
}

fn summary_view(app: &App, width: f32, height: f32) -> Node {
    let body_h = (screen_h(height) - FLEET_HEADER_H - FLEET_PAD * 2.0).max(160.0);
    let outer = (width - FLEET_PAD * 2.0).max(320.0);
    let route_w = 300.0;
    let table_w = (outer - route_w - 10.0).max(520.0);
    row(
        props!(width: outer, height: body_h, gap: 10.0),
        [
            summary_table(app, table_w, body_h),
            summary_routes(app, route_w, body_h),
        ],
    )
}

fn summary_table(app: &App, width: f32, height: f32) -> Node {
    let inner = (width - 24.0).max(200.0);
    let miners: Vec<&Miner> = hash_order_indexes(&app.miners, &app.hash_order)
        .into_iter()
        .map(|index| &app.miners[index])
        .collect();
    let scroll_h = (height - 176.0).max(80.0);
    let layout_w = (inner - 20.0).max(180.0);
    let layout = summary_layout(layout_w, &app.summary_cols);
    let mut rows = Vec::new();
    for miner in miners {
        rows.push(summary_miner_row(miner, &layout, &app.summary_cols));
    }
    col(
        props!(
            width: width,
            height: height,
            background: Color::from_rgb(22, 24, 28),
            border_radius: 16.0,
            padding: 12.0,
            gap: 4.0,
        ),
        [
            text(
                "Detailed Summary",
                style!(size: 34, weight: FontWeight::BOLD, color: EMERALD, line_height: 1.0),
            ),
            summary_header(app, &layout),
            scroll("summary-scroll", props!(width: inner, height: scroll_h), rows),
            summary_rule(layout.row),
            summary_totals_line(app, &layout),
        ],
    )
}

fn summary_header(app: &App, layout: &SummaryLayout) -> Node {
    let mut cells = vec![
        col(props!(width: 18.0, height: 18.0), Vec::<Node>::new()),
        summary_table_cell("Miner", LABEL, layout.name, true, false),
    ];
    for (index, col_id) in app.summary_cols.iter().enumerate() {
        let leading = *col_id % SUMMARY_COL_COUNT == 0;
        let slot = summary_slot_width(*col_id, layout);
        cells.push(col(
            props!(width: slot, height: 48.0),
            [
                summary_table_cell(summary_col_title(*col_id), EMERALD, slot, leading, true),
                touchable(
                    &format!("sum-{index}"),
                    props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                    Vec::<Draw>::new(),
                ),
            ],
        ));
    }
    row(
        props!(width: layout.row, height: 48.0, gap: 8.0, cross_align: CrossAlign::Center),
        cells,
    )
}

fn summary_miner_row(miner: &Miner, layout: &SummaryLayout, cols: &[u8; 4]) -> Node {
    let dot = if miner.reachable {
        temp_color(miner.temp_c, miner.family)
    } else {
        MUTED
    };
    let mut cells = vec![
        canvas(props!(width: 18.0, height: 18.0), [Draw::circle(9.0, 9.0, 7.0, dot)]),
        summary_table_cell(&miner_title(miner), WHITE, layout.name, true, false),
    ];
    for col_id in cols {
        let (value, color) = summary_metric(miner, *col_id);
        let leading = *col_id % SUMMARY_COL_COUNT == 0;
        cells.push(summary_table_cell(
            &value,
            color,
            summary_slot_width(*col_id, layout),
            leading,
            false,
        ));
    }
    row(
        props!(width: layout.row, height: 54.0, gap: 8.0, cross_align: CrossAlign::Center),
        cells,
    )
}

fn summary_totals_line(app: &App, layout: &SummaryLayout) -> Node {
    let mut cells = vec![
        canvas(
            props!(width: 18.0, height: 18.0),
            [Draw::circle(9.0, 9.0, 7.0, EMERALD)],
        ),
        summary_table_cell("Totals", EMERALD, layout.name, true, true),
    ];
    for col_id in app.summary_cols {
        let (value, color) = summary_totals_metric(app, col_id);
        let leading = col_id % SUMMARY_COL_COUNT == 0;
        cells.push(summary_table_cell(
            &value,
            color,
            summary_slot_width(col_id, layout),
            leading,
            true,
        ));
    }
    row(
        props!(width: layout.row, height: 52.0, gap: 8.0, cross_align: CrossAlign::Center),
        cells,
    )
}

fn summary_rule(width: f32) -> Node {
    canvas(
        props!(width: width, height: 10.0),
        [Draw::rect(0.0, 4.0, width, 1.0, Color::from_rgba(255, 255, 255, 70))],
    )
}

fn summary_routes(app: &App, width: f32, height: f32) -> Node {
    let inner = (width - 24.0).max(160.0);
    col(
        props!(
            width: width,
            height: height,
            background: Color::from_rgb(22, 24, 28),
            border_radius: 16.0,
            padding: 12.0,
            gap: 8.0,
        ),
        [
            text(
                "Pool Routing",
                style!(size: 28, weight: FontWeight::BOLD, color: EMERALD, line_height: 1.0),
            ),
            scroll(
                "route-scroll",
                props!(width: inner, height: (height - 52.0).max(80.0)),
                pool_routes(app, inner),
            ),
        ],
    )
}

fn pool_routes(app: &App, width: f32) -> Vec<Node> {
    struct Route {
        url: String,
        hashrate: f64,
        zec: bool,
        count: usize,
    }
    let mut routes: Vec<Route> = Vec::new();
    for miner in &app.miners {
        if !miner.reachable || miner.hashrate_ths.unwrap_or(0.0) <= 0.0 {
            continue;
        }
        let url = short_pool(&miner.pool);
        let zec = miner.family == "bitmainZEC";
        if let Some(route) = routes.iter_mut().find(|route| route.url == url && route.zec == zec) {
            route.hashrate += miner.hashrate_ths.unwrap_or(0.0);
            route.count += 1;
        } else {
            routes.push(Route {
                url,
                hashrate: miner.hashrate_ths.unwrap_or(0.0),
                zec,
                count: 1,
            });
        }
    }
    routes.sort_by(|left, right| right.hashrate.partial_cmp(&left.hashrate).unwrap_or(std::cmp::Ordering::Equal));
    let sha_max = routes.iter().filter(|route| !route.zec).map(|route| route.hashrate).fold(0.0, f64::max);
    let zec_max = routes.iter().filter(|route| route.zec).map(|route| route.hashrate).fold(0.0, f64::max);
    let mut nodes = Vec::new();
    for route in routes {
        let max = if route.zec { zec_max } else { sha_max };
        let fraction = if max > 0.0 { (route.hashrate / max) as f32 } else { 0.0 };
        let color = if route.zec { SHARE_ORANGE } else { EMERALD };
        let hash = if route.zec {
            format!("{:.1} KSol/s", route.hashrate)
        } else {
            format_hashrate(Some(route.hashrate))
        };
        let devices = if route.count == 1 {
            "1 device routing here".to_owned()
        } else {
            format!("{} devices routing here", route.count)
        };
        let card = if route.zec {
            Color::from_rgb(48, 28, 14)
        } else {
            Color::from_rgb(12, 42, 28)
        };
        let bar_w = (width - 24.0).max(80.0);
        nodes.push(col(
            props!(
                width: width,
                background: card,
                border_radius: 14.0,
                padding: 12.0,
                gap: 6.0,
            ),
            [
                row(
                    props!(width: bar_w, justify_content: Justify::SpaceBetween, cross_align: CrossAlign::Center),
                    [
                        row(
                            props!(gap: 8.0, cross_align: CrossAlign::Center),
                            [
                                canvas(
                                    props!(width: 14.0, height: 14.0),
                                    [Draw::circle(7.0, 7.0, 5.0, color)],
                                ),
                                text(
                                    route.url,
                                    style!(
                                        size: 22,
                                        weight: FontWeight::SEMIBOLD,
                                        color: WHITE,
                                        line_height: 1.0,
                                        text_overflow: TextOverflow::Ellipsis,
                                        max_width: (bar_w * 0.58) as u32,
                                    ),
                                ),
                            ],
                        ),
                        text(
                            hash,
                            style!(size: 24, weight: FontWeight::BOLD, color: color, line_height: 1.0),
                        ),
                    ],
                ),
                text(devices, style!(size: 18, color: LABEL, line_height: 1.0)),
                canvas(
                    props!(width: bar_w, height: 12.0),
                    [
                        Draw::rect(0.0, 2.0, bar_w, 8.0, Color::from_rgba(255, 255, 255, 36)),
                        Draw::rect(0.0, 2.0, (bar_w * fraction).max(8.0), 8.0, color),
                    ],
                ),
            ],
        ));
    }
    if nodes.is_empty() {
        nodes.push(text(
            "No pool routes yet",
            style!(size: 20, color: LABEL, line_height: 1.0),
        ));
    }
    nodes
}

fn fleet_view(app: &App, width: f32, height: f32) -> Node {
    let height = screen_h(height);
    if app.neural && !app.summary && !app.miners.is_empty() {
        return with_theme(width, height, neural_board(app, width, height));
    }
    let numbers = fleet_numbers(app);
    let share_color = if app.share_flash_ms > 0 { Color::from_rgb(255, 210, 60) } else { SHARE_ORANGE };
    let (hash, zec_hash) = fleet_hash_lines(numbers.sha, numbers.zec);
    let fleet_on = !app.neural && !app.summary;
    let header_controls = row(
        props!(height: FLEET_HEADER_H, gap: 0.0, cross_align: CrossAlign::Center),
        [
            view_segment("view-fleet", "Fleet", fleet_on),
            view_edge(),
            view_segment("view-neural", "Neural", app.neural),
            view_edge(),
            view_segment("view-summary", "Summary", app.summary),
            view_edge(),
            col(
                props!(
                    width: 64.0,
                    height: FLEET_HEADER_H,
                    justify_content: Justify::Center,
                    cross_align: CrossAlign::Center,
                ),
                [gear_button()],
            ),
        ],
    );
    let bar_w = (width - 20.0).max(80.0);
    let metrics = row(
        props!(
            width: width,
            height: FLEET_METRICS_H,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [col(
            props!(
                height: FLEET_METRICS_H - 8.0,
                width: bar_w,
                background: Color::from_rgba(28, 32, 30, 235),
                border_radius: 18.0,
                border_width: 1.5,
                border_color: Color::from_rgba(150, 158, 152, 170),
            ),
            [row(
                props!(height: FLEET_METRICS_H - 8.0, gap: 0.0),
                [
                    hash_metric(&hash, zec_hash.as_deref(), bar_w / 6.0, FLEET_METRICS_H - 8.0),
                    metric_widget("Shares", &numbers.shares, share_color, bar_w / 6.0, FLEET_METRICS_H - 8.0),
                    metric_widget("Avg Temp", &numbers.temp, WHITE, bar_w / 6.0, FLEET_METRICS_H - 8.0),
                    metric_widget("Avg Eff", &numbers.eff, WHITE, bar_w / 6.0, FLEET_METRICS_H - 8.0),
                    metric_widget("Total Power", &numbers.power, WHITE, bar_w / 6.0, FLEET_METRICS_H - 8.0),
                    metric_widget("Online", &numbers.online, WHITE, bar_w / 6.0, FLEET_METRICS_H - 8.0),
                ],
            )],
        )],
    );
    let summary_open = app.summary && !app.miners.is_empty();
    let neural_open = app.neural && !app.summary && !app.miners.is_empty();
    let lower = if summary_open {
        summary_view(app, width, height)
    } else if neural_open {
        neural_board(app, width, height)
    } else {
        fleet_list(app, width, height)
    };
    let list = col(
        props!(
            width: width,
            padding: FLEET_PAD,
            gap: FLEET_GAP,
        ),
        [lower],
    );
    let mut stack = vec![col(
        props!(height: FLEET_HEADER_H, width: width, background: BAR_FILL),
        [
            row(
                props!(
                    inset_top: 0.0,
                    inset_right: 0.0,
                    inset_bottom: 0.0,
                    inset_left: 0.0,
                    justify_content: Justify::Center,
                    cross_align: CrossAlign::Center,
                    gap: 18.0,
                ),
                neural_clock_group(app, &hash, zec_hash.as_deref(), &numbers.shares),
            ),
            row(
                props!(
                    height: FLEET_HEADER_H,
                    width: width,
                    justify_content: Justify::SpaceBetween,
                    cross_align: CrossAlign::Center,
                    padding: 0.0,
                ),
                [
                    row(
                        props!(gap: 12.0, cross_align: CrossAlign::Center),
                        [
                            canvas(props!(width: 12.0, height: 1.0), Vec::<Draw>::new()),
                            wordmark(42),
                        ],
                    ),
                    header_controls,
                ],
            ),
        ],
    )];
    if !neural_open && !summary_open {
        stack.push(metrics);
    }
    stack.push(list);
    with_theme(
        width,
        height,
        col(
            props!(background: TRANSPARENT, width: width, height: height, gap: 0.0),
            stack,
        ),
    )
}

enum FleetBand {
    Groups(Vec<(&'static str, Vec<usize>)>),
    Cards(Vec<usize>),
}

fn fleet_bands(miners: &[Miner]) -> Vec<FleetBand> {
    let mut families: Vec<&'static str> = Vec::new();
    for miner in miners {
        if !families.contains(&miner.family) {
            families.push(miner.family);
        }
    }
    families.sort_by_key(|family| (family_rank(family), *family));
    let mut groups = Vec::new();
    for family in families {
        let indexes: Vec<usize> = miners
            .iter()
            .enumerate()
            .filter(|(_, miner)| miner.family == family)
            .map(|(index, _)| index)
            .collect();
        if !indexes.is_empty() {
            groups.push((family, indexes));
        }
    }
    groups
        .chunks(2)
        .map(|chunk| FleetBand::Groups(chunk.to_vec()))
        .collect()
}

fn hash_rank(miner: &Miner) -> f64 {
    if miner.reachable {
        miner.hashrate_ths.unwrap_or(0.0)
    } else {
        -1.0
    }
}

fn hash_order_indexes(miners: &[Miner], order: &[String]) -> Vec<usize> {
    let mut indexes = Vec::new();
    for ip in order {
        if let Some(index) = miners.iter().position(|miner| miner.ip == *ip) {
            indexes.push(index);
        }
    }
    for (index, miner) in miners.iter().enumerate() {
        if !order.iter().any(|ip| ip == &miner.ip) {
            indexes.push(index);
        }
    }
    indexes
}

fn fleet_hash_rows(miners: &[Miner], order: &[String]) -> Vec<FleetBand> {
    hash_order_indexes(miners, order)
        .chunks(FLEET_COLUMNS)
        .map(|chunk| FleetBand::Cards(chunk.to_vec()))
        .collect()
}

fn fleet_rows(app: &App) -> Vec<FleetBand> {
    if app.fleet_groups {
        fleet_bands(&app.miners)
    } else {
        fleet_hash_rows(&app.miners, &app.hash_order)
    }
}

fn band_height(band: &FleetBand) -> f32 {
    match band {
        FleetBand::Groups(_) => FLEET_GROUP_H,
        FleetBand::Cards(_) => FLEET_CARD_H,
    }
}

fn fleet_fit_count(bands: &[FleetBand], start: usize, list_h: f32) -> usize {
    let mut used = 0.0;
    let mut count = 0;
    for band in bands.iter().skip(start) {
        let next = used + band_height(band) + if count == 0 { 0.0 } else { FLEET_GAP };
        if count > 0 && next > list_h {
            break;
        }
        used = next;
        count += 1;
    }
    count.max(1).min(bands.len().saturating_sub(start))
}

fn fleet_max_band(bands: &[FleetBand], list_h: f32) -> usize {
    let mut used = 0.0;
    let mut count = 0;
    for band in bands.iter().rev() {
        let next = used + band_height(band) + if count == 0 { 0.0 } else { FLEET_GAP };
        if count > 0 && next > list_h {
            break;
        }
        used = next;
        count += 1;
    }
    bands.len().saturating_sub(count.max(1))
}

fn fleet_group_banner(miners: &[Miner], family: &str, indexes: &[usize], width: f32, height: f32, flash: u8) -> Node {
    let members: Vec<&Miner> = indexes.iter().filter_map(|index| miners.get(*index)).collect();
    let mut hashrate = 0.0;
    let mut power = 0.0;
    let mut shares = 0_u64;
    let mut best = 0.0;
    let mut temps = Vec::new();
    for miner in &members {
        hashrate += miner.hashrate_ths.unwrap_or(0.0);
        power += miner.power_w.unwrap_or(0.0);
        shares = shares.saturating_add(miner.shares_accepted.unwrap_or(0));
        if let Some(diff) = miner.best_diff {
            if diff > best && diff < 1.0e15 {
                best = diff;
            }
        }
        temps.push(format_temp(miner.temp_c));
    }
    let hash_text = if family == "bitmainZEC" {
        if hashrate > 0.0 {
            format!("{hashrate:.0} KSol/s")
        } else {
            "—".to_owned()
        }
    } else {
        format_hashrate(Some(hashrate).filter(|value| *value > 0.0))
    };
    let temp_line = if temps.is_empty() { "—".to_owned() } else { temps.join("   ") };
    let best_text = format!("Best {}", format_difficulty(Some(best).filter(|value| *value > 0.0)));
    let power_text = format_power(Some(power).filter(|value| *value > 0.0));
    let share_text = count_text(Some(shares).filter(|value| *value > 0));
    let online = members.iter().filter(|miner| miner.reachable).count();
    let hashing = members.iter().any(|miner| miner.reachable && miner.hashrate_ths.unwrap_or(0.0) > 0.0);
    let edge = if flash > 0 {
        Color::from_rgba(255, 159, 10, flash)
    } else if hashing {
        GREEN
    } else if online > 0 {
        AMBER
    } else {
        RED
    };
    let count_label = if members.len() == 1 { "1 miner".to_owned() } else { format!("{} miners", members.len()) };
    col(
        props!(
            width: width,
            height: height,
            background: CARD,
            border_radius: 12.0,
            border_width: 1.5,
            border_color: edge,
            padding: 12.0,
            gap: 4.0,
        ),
        [
            row(
                props!(width: (width - 24.0).max(40.0), justify_content: Justify::SpaceBetween, cross_align: CrossAlign::Center),
                [
                    clipped(family_label(family), 30, FontWeight::BOLD, WHITE, width * 0.62),
                    text(
                        format!("{online} online"),
                        style!(size: 22, weight: FontWeight::SEMIBOLD, color: if online > 0 { EMERALD } else { RED }, line_height: 1.0),
                    ),
                ],
            ),
            text(count_label, style!(size: 18, weight: FontWeight::REGULAR, color: LABEL, line_height: 1.0)),
            text(
                hash_text,
                style!(size: 40, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: EMERALD, line_height: 1.0),
            ),
            clipped(
                format!("{temp_line}   {power_text}   {best_text}   {share_text}"),
                20,
                FontWeight::SEMIBOLD,
                WHITE,
                width - 24.0,
            ),
        ],
    )
}

fn fleet_list(app: &App, width: f32, height: f32) -> Node {
    let list_h = fleet_list_height(height);
    if app.miners.is_empty() {
        let ip = bmc_wasm_sdk::network::info().ip;
        let ip_shown = if ip.is_empty() { "—".to_owned() } else { ip };
        return row(
            props!(height: list_h, gap: 28.0, cross_align: CrossAlign::Center),
            [
                qr_tile(200.0, &DOWNLOAD_QR),
                col(
                    props!(gap: 6.0, width: (width - 260.0).max(360.0)),
                    [
                        text(
                            "Congrats. HashWatcher is on your Deck.",
                            style!(size: 26, weight: FontWeight::BOLD, color: WHITE, line_height: 1.0),
                        ),
                        text(
                            ip_shown,
                            style!(size: 48, weight: FontWeight::BOLD, color: EMERALD, line_height: 1.0),
                        ),
                        text(
                            "Look for this IP in the scan results.",
                            style!(size: 22, weight: FontWeight::BOLD, color: WHITE, line_height: 1.1),
                        ),
                        text(
                            "Add Miner, then Scan All. Scan the code to install the app.",
                            style!(size: 18, color: LABEL, line_height: 1.2),
                        ),
                    ],
                ),
            ],
        );
    }
    let bands = fleet_rows(app);
    let card_w = fleet_card_width(width);
    let tile_w = ((width - FLEET_PAD * 2.0 - FLEET_GAP) / 2.0).max(40.0);
    let max_start = fleet_max_band(&bands, list_h);
    let start = if max_start > 0 {
        app.fleet_row.min(max_start)
    } else {
        0
    };
    let shown = fleet_fit_count(&bands, start, list_h);
    let mut children = Vec::new();
    for band in bands.iter().skip(start).take(shown) {
        match band {
            FleetBand::Groups(groups) => {
                let mut tiles = Vec::new();
                for (family, indexes) in groups {
                    tiles.push(fleet_group_banner(&app.miners, family, indexes, tile_w, FLEET_GROUP_H, 0));
                }
                children.push(row(props!(gap: FLEET_GAP), tiles));
            }
            FleetBand::Cards(indexes) => {
                let mut row_cards = Vec::new();
                for index in indexes {
                    if let Some(miner) = app.miners.get(*index) {
                        row_cards.push(miner_card(miner, card_w, max_start == 0, 0));
                    }
                }
                children.push(row(props!(gap: FLEET_GAP), row_cards));
            }
        }
    }
    if max_start > 0 {
        let hint = if start > 0 && start < max_start {
            "Drag for more miners"
        } else if start < max_start {
            "Drag up for more miners"
        } else {
            "Drag down for previous miners"
        };
        children.push(text(hint, style!(size: 20, color: MUTED, line_height: 1.1)));
        children.push(touchable(
            "fleet-hit",
            props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
            Vec::<Draw>::new(),
        ));
    }
    col(props!(height: list_h, gap: FLEET_GAP), children)
}

fn miner_card(miner: &Miner, width: f32, tappable: bool, flash: u8) -> Node {
    let id = format!("miner-{}", miner.ip);
    let right = if miner.reachable {
        text(
            format_uptime(miner.uptime_s),
            style!(size: 20, weight: FontWeight::SEMIBOLD, color: EMERALD, line_height: 1.0),
        )
    } else {
        text("Offline", style!(size: 20, weight: FontWeight::SEMIBOLD, color: RED, line_height: 1.0))
    };
    let mut children = vec![
        row(
            props!(justify_content: Justify::SpaceBetween, cross_align: CrossAlign::Center),
            [
                clipped(&miner_title(miner), 26, FontWeight::BOLD, WHITE, width * 0.62),
                right,
            ],
        ),
        clipped(
            format!("{}  |  {}", family_label(miner.family), miner.ip),
            18,
            FontWeight::REGULAR,
            LABEL,
            width - 12.0,
        ),
        text(
            display_hashrate(miner),
            style!(size: 32, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: EMERALD, line_height: 1.0),
        ),
        clipped(
            format!(
                "{}   {}   {}",
                format_temp(miner.temp_c),
                format_power(miner.power_w),
                format_fan(miner.fan.or(miner.fan_rpm)),
            ),
            20,
            FontWeight::SEMIBOLD,
            WHITE,
            width - 16.0,
        ),
    ];
    if tappable {
        children.push(touchable(
            &id,
            props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
            Vec::<Draw>::new(),
        ));
    }
    col(
        props!(
            width: width,
            height: FLEET_CARD_H,
            background: CARD,
            border_radius: 12.0,
            border_width: 1.5,
            border_color: if flash > 0 { Color::from_rgba(255, 159, 10, flash) } else { status_color(miner) },
            padding: 8.0,
            gap: 2.0,
        ),
        children,
    )
}

fn dashboard(miner: &Miner, app: &App, width: f32, height: f32) -> Node {
    let height = screen_h(height);
    let gap = 8.0;
    let header_h = 56.0;
    let body_h = (height - FLEET_PAD * 2.0 - header_h - gap).max(200.0);
    let inner = (width - FLEET_PAD * 2.0).max(100.0);
    let usable = inner - gap * 2.0;
    let left_w = usable * 0.42;
    let mid_w = usable * 0.29;
    let right_w = usable - left_w - mid_w;
    with_theme(
        width,
        height,
        col(
        props!(background: TRANSPARENT, width: width, height: height, padding: FLEET_PAD, gap: gap),
        [
            dashboard_header(miner, app, inner),
            row(
                props!(height: body_h, gap: gap),
                [
                    hashrate_column(miner, left_w, body_h),
                    temperature_column(miner, mid_w, body_h),
                    pool_column(miner, right_w, body_h),
                ],
            ),
        ],
        ),
    )
}

fn dashboard_header(miner: &Miner, app: &App, width: f32) -> Node {
    let (status, color) = if app.notice.is_empty() {
        (
            format!("{}   {}", family_label(miner.family), status_label(miner)),
            status_color(miner),
        )
    } else {
        (app.notice.clone(), AMBER)
    };
    row(
        props!(
            height: 56.0,
            justify_content: Justify::SpaceBetween,
            cross_align: CrossAlign::Center,
        ),
        [
            row(
                props!(gap: 10.0, cross_align: CrossAlign::Center),
                [
                    back_button("back"),
                    clipped(
                        format!("{}  |  {}", miner_title(miner), miner.ip),
                        20,
                        FontWeight::BOLD,
                        WHITE,
                        (width - 280.0).max(80.0),
                    ),
                ],
            ),
            row(
                props!(gap: 8.0, cross_align: CrossAlign::Center),
                [
                    clipped(status, 15, FontWeight::SEMIBOLD, color, 200.0),
                    gear_button(),
                ],
            ),
        ],
    )
}

fn stat_rows(miner: &Miner, efficiency: Option<f64>, inner_w: f32, grid_h: f32) -> Vec<Node> {
    let mut items: Vec<(&str, String, Color)> = Vec::new();
    if miner.family != "bitmainZEC" {
        items.push(("Best", format_difficulty(miner.best_diff), AMBER));
        items.push(("Session", format_difficulty(miner.best_session), AMBER));
    }
    items.push(("Shares", count_text(miner.shares_accepted), WHITE));
    if miner.power_w.is_some() {
        items.push(("Power", format_power(miner.power_w), WHITE));
    }
    if efficiency.is_some() {
        items.push(("Efficiency", format_efficiency(efficiency), WHITE));
    }
    items.push(("Uptime", format_uptime(miner.uptime_s), EMERALD));
    let row_count = items.len().div_ceil(3).max(1) as f32;
    let tile_h = (grid_h - 8.0 * (row_count - 1.0)) / row_count;
    let mut nodes = Vec::new();
    for chunk in items.chunks(3) {
        let tile_w = (inner_w - 6.0 * (chunk.len() as f32 - 1.0).max(0.0)) / chunk.len() as f32;
        nodes.push(row(
            props!(gap: 6.0),
            chunk
                .iter()
                .map(|(label, value, color)| stat_tile(label, value, *color, tile_w, tile_h))
                .collect::<Vec<_>>(),
        ));
    }
    nodes
}

fn zec_fan_percent(rpm: f64, peak: f64) -> f64 {
    let ceiling = 6_000.0_f64.max(peak).max(rpm);
    if ceiling <= 0.0 {
        return 0.0;
    }
    ((rpm / ceiling) * 100.0).round().clamp(1.0, 100.0)
}

fn zec_scrambled(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("_ant_") || lower.contains("=_ant_") || (lower.contains("bitmain-") && lower.contains('='))
}

fn zec_fan_form(conf: &str, manual: bool, percent: i32) -> Option<String> {
    let doc = JsonDoc::parse(conf.as_bytes());
    let mut slots = Vec::new();
    for index in 0..3 {
        let url = doc.str(&format!("/pools/{index}/url")).unwrap_or_default();
        let user = doc.str(&format!("/pools/{index}/user")).unwrap_or_default();
        let pass = doc.str(&format!("/pools/{index}/pass")).unwrap_or_default();
        if zec_scrambled(&url) || zec_scrambled(&user) || zec_scrambled(&pass) {
            return None;
        }
        slots.push((url, user, pass));
    }
    if !slots.iter().any(|(url, _, _)| {
        let trimmed = url.trim();
        !trimmed.is_empty() && (trimmed.contains('.') || trimmed.to_ascii_lowercase().contains("stratum"))
    }) {
        return None;
    }
    let freq = doc.str("/bitmain-freq").unwrap_or_default();
    let raw_voltage = doc.str("/bitmain-voltage").unwrap_or_default();
    if zec_scrambled(&freq) || zec_scrambled(&raw_voltage) {
        return None;
    }
    let voltage = if zec_voltage_ok(&raw_voltage) { raw_voltage } else { String::new() };
    let mut fields = Vec::new();
    for (index, (url, user, pass)) in slots.iter().enumerate() {
        let slot = index + 1;
        fields.push((format!("_ant_pool{slot}url"), url.clone()));
        fields.push((format!("_ant_pool{slot}user"), user.clone()));
        fields.push((format!("_ant_pool{slot}pw"), pass.clone()));
    }
    fields.push(("_ant_nobeeper".to_owned(), "false".to_owned()));
    fields.push(("_ant_notempoverctrl".to_owned(), "false".to_owned()));
    fields.push((
        "_ant_fan_customize_switch".to_owned(),
        if manual { "true".to_owned() } else { "false".to_owned() },
    ));
    fields.push(("_ant_fan_customize_value".to_owned(), percent.clamp(0, 100).to_string()));
    fields.push(("_ant_freq".to_owned(), freq));
    fields.push(("_ant_voltage".to_owned(), voltage));
    Some(
        fields
            .into_iter()
            .map(|(key, value)| format!("{}={}", form_escape(&key), form_escape(&value)))
            .collect::<Vec<_>>()
            .join("&"),
    )
}

fn zec_voltage_ok(value: &str) -> bool {
    let hex = value.trim().trim_start_matches("0x").trim_start_matches("0X");
    !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) && hex.bytes().any(|byte| byte != b'0')
}

fn form_escape(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn read_digest_challenge(header: &str, job: &mut ZecFanJob) -> bool {
    let Some(rest) = header.split_once("igest") else {
        return false;
    };
    let mut realm = String::new();
    let mut nonce = String::new();
    let mut qop = false;
    for part in rest.1.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_start_matches("Digest").trim();
        let value = value.trim().trim_matches('"');
        match key.to_ascii_lowercase().as_str() {
            "realm" => realm = value.to_owned(),
            "nonce" => nonce = value.to_owned(),
            "qop" => qop = value.to_ascii_lowercase().contains("auth"),
            _ => {}
        }
    }
    if realm.is_empty() || nonce.is_empty() {
        return false;
    }
    job.realm = realm;
    job.nonce = nonce;
    job.qop = qop;
    true
}

fn digest_header(user: &str, pass: &str, realm: &str, nonce: &str, qop: bool, method: &str, uri: &str) -> String {
    let ha1 = md5_hex(format!("{user}:{realm}:{pass}").as_bytes());
    let ha2 = md5_hex(format!("{method}:{uri}").as_bytes());
    let cnonce = "hashwatcher";
    let response = if qop {
        md5_hex(format!("{ha1}:{nonce}:00000001:{cnonce}:auth:{ha2}").as_bytes())
    } else {
        md5_hex(format!("{ha1}:{nonce}:{ha2}").as_bytes())
    };
    if qop {
        format!(
            "Authorization: Digest username=\"{user}\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{uri}\", algorithm=MD5, qop=auth, nc=00000001, cnonce=\"{cnonce}\", response=\"{response}\""
        )
    } else {
        format!(
            "Authorization: Digest username=\"{user}\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{uri}\", algorithm=MD5, response=\"{response}\""
        )
    }
}

fn md5_hex(data: &[u8]) -> String {
    md5(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn md5(message: &[u8]) -> [u8; 16] {
    fn f(x: u32, y: u32, z: u32) -> u32 { (x & y) | (!x & z) }
    fn g(x: u32, y: u32, z: u32) -> u32 { (x & z) | (y & !z) }
    fn h(x: u32, y: u32, z: u32) -> u32 { x ^ y ^ z }
    fn i(x: u32, y: u32, z: u32) -> u32 { y ^ (x | !z) }
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
        14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
        21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501, 0x698098d8,
        0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340,
        0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87,
        0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039,
        0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92,
        0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let bit_len = (message.len() as u64).wrapping_mul(8);
    let mut data = message.to_vec();
    data.push(0x80);
    while (data.len() % 64) != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_le_bytes());
    let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in data.chunks(64) {
        let mut words = [0u32; 16];
        for (index, word) in words.iter_mut().enumerate() {
            let offset = index * 4;
            *word = u32::from_le_bytes([chunk[offset], chunk[offset + 1], chunk[offset + 2], chunk[offset + 3]]);
        }
        let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
        for step in 0..64 {
            let (value, index) = match step {
                0..=15 => (f(b, c, d), step),
                16..=31 => (g(b, c, d), (5 * step + 1) % 16),
                32..=47 => (h(b, c, d), (3 * step + 5) % 16),
                _ => (i(b, c, d), (7 * step) % 16),
            };
            let next = b.wrapping_add(
                a.wrapping_add(value)
                    .wrapping_add(K[step])
                    .wrapping_add(words[index])
                    .rotate_left(S[step]),
            );
            a = d;
            d = c;
            c = b;
            b = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn hashrate_column(miner: &Miner, width: f32, height: f32) -> Node {
    let (value, unit) = if miner.family == "bitmainZEC" {
        match miner.hashrate_ths {
            Some(rate) if rate > 0.0 => (format!("{rate:.2}"), "KSol/s".to_owned()),
            _ => ("—".to_owned(), String::new()),
        }
    } else {
        hashrate_parts(miner.hashrate_ths)
    };
    let efficiency = match (miner.power_w, miner.hashrate_ths) {
        (Some(power), Some(hashrate)) if hashrate > 0.0 => Some(power / hashrate),
        _ => None,
    };
    let inner_w = (width - 24.0).max(60.0);
    let grid_h = (height - 168.0).max(96.0);
    card(
        "Hashrate",
        width,
        height,
        EMERALD,
        vec![
            row(
                props!(cross_align: CrossAlign::Center, gap: 8.0),
                [
                    text(
                        value,
                        style!(size: 48, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: EMERALD),
                    ),
                    text(
                        unit,
                        style!(size: 22, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: EMERALD),
                    ),
                ],
            ),
            row(
                props!(gap: 14.0),
                {
                    let mut chips = vec![text(
                        format!("Chip {}", format_temp(miner.temp_c)),
                        style!(size: 16, weight: FontWeight::SEMIBOLD, color: temp_color(miner.temp_c, miner.family)),
                    )];
                    if miner.family == "bitmainZEC" {
                        if let Some(board) = miner.board_temp {
                            chips.push(text(
                                format!("PCB {}", format_temp(Some(board))),
                                style!(size: 16, weight: FontWeight::SEMIBOLD, color: BLUE),
                            ));
                        }
                    } else if miner.vr_temp.is_some() {
                        chips.push(text(
                            format!("VR {}", format_temp(miner.vr_temp)),
                            style!(size: 16, weight: FontWeight::SEMIBOLD, color: BLUE),
                        ));
                    }
                    chips.push(text(
                        format_mhz(miner.frequency),
                        style!(size: 16, weight: FontWeight::SEMIBOLD, color: WHITE),
                    ));
                    chips
                },
            ),
            col(
                props!(height: grid_h, gap: 8.0),
                stat_rows(miner, efficiency, inner_w, grid_h),
            ),
        ],
    )
}

fn temperature_column(miner: &Miner, width: f32, height: f32) -> Node {
    let gap = 8.0;
    let chip_h = 148.0;
    let fan_h = (height - gap - chip_h).max(160.0);
    col(
        props!(width: width, height: height, gap: gap),
        [
            packed_card(
                "Chip Temperature",
                width,
                chip_h,
                Color::from_hex(0x2A_4A_38),
                {
                    let mut lines = vec![text(
                        format_temp(miner.temp_c),
                        style!(
                            size: 48,
                            weight: FontWeight::BOLD,
                            family: FontFamily::DeckSans,
                            color: temp_color(miner.temp_c, miner.family),
                            line_height: 1.0,
                        ),
                    )];
                    if miner.family == "bitmainZEC" {
                        if let Some(board) = miner.board_temp {
                            lines.push(text(
                                format!("PCB  {}", format_temp(Some(board))),
                                style!(size: 22, weight: FontWeight::BOLD, color: BLUE, line_height: 1.0),
                            ));
                        }
                    } else if let Some(vr) = miner.vr_temp {
                        lines.push(text(
                            format!("VR  {}", format_temp(Some(vr))),
                            style!(size: 22, weight: FontWeight::BOLD, color: BLUE, line_height: 1.0),
                        ));
                    }
                    lines.push(text(
                        format!("{}    {}", format_mhz(miner.frequency), format_mv(miner.voltage)),
                        style!(size: 15, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0),
                    ));
                    lines
                },
            ),
            fan_card(miner, width, fan_h),
        ],
    )
}

fn fan_card(miner: &Miner, width: f32, height: f32) -> Node {
    let percent = miner.fan.filter(|value| *value <= 100.0);
    let rpm = miner.fan_rpm.or_else(|| miner.fan.filter(|value| *value > 100.0));
    let mut body = vec![text(
        format!("{}    {}", format_fan(percent), format_fan(rpm)),
        style!(size: 32, weight: FontWeight::BOLD, family: FontFamily::DeckSans, color: WHITE),
    )];
    if shows_fan_controls(miner) {
        body.push(row(
            props!(gap: 8.0, width: (width - 28.0).max(80.0)),
            [
                compact_pill("fan-auto", "Auto", !miner.fan_manual),
                compact_pill("fan-manual", "Manual", miner.fan_manual),
            ],
        ));
        if miner.fan_manual {
            body.push(row(
                props!(gap: 6.0, width: (width - 28.0).max(80.0)),
                [
                    compact_pill("fan-40", "40%", false),
                    compact_pill("fan-60", "60%", false),
                    compact_pill("fan-80", "80%", false),
                    compact_pill("fan-100", "100%", false),
                ],
            ));
        }
    }
    let reserved = if shows_fan_controls(miner) {
        if miner.fan_manual { 168.0 } else { 128.0 }
    } else {
        88.0
    };
    let gauge_h = (height - reserved).max(96.0);
    body.push(fan_rings(miner, (width - 20.0).max(80.0), gauge_h));
    packed_card("Fans", width, height, BLUE, body)
}

fn fan_samples(miner: &Miner) -> Vec<(f64, f64, u8)> {
    let mut samples = Vec::new();
    for index in 0..miner.fan_pcts.len() {
        let pct = miner.fan_pcts[index];
        let rpm = miner.fan_rpms[index];
        if pct > 0.0 || rpm > 0.0 {
            samples.push((pct, rpm, index as u8 + 1));
        }
    }
    if samples.is_empty() {
        let pct = miner.fan.filter(|value| *value > 0.0 && *value <= 100.0).unwrap_or(0.0);
        let rpm = miner.fan_rpm.unwrap_or(0.0);
        if pct > 0.0 || rpm > 0.0 {
            samples.push((pct, rpm, 1));
        }
    }
    samples
}

fn fan_rings(miner: &Miner, width: f32, height: f32) -> Node {
    let samples = fan_samples(miner);
    if samples.is_empty() {
        return col(props!(width: width, height: 1.0), Vec::<Node>::new());
    }
    let count = samples.len() as f32;
    let slot = width / count;
    let radius = (slot.min(height) * 0.34).clamp(22.0, 46.0);
    let mut draws = Vec::new();
    for (index, (pct, rpm, number)) in samples.iter().enumerate() {
        let cx = slot * (index as f32 + 0.5);
        let cy = radius + 8.0;
        draws.extend(fan_ring(cx, cy, radius, *pct));
        let label_y = cy + radius + 16.0;
        draws.push(Draw::text(
            cx,
            label_y,
            if samples.len() == 1 {
                "Fan".to_owned()
            } else {
                format!("Fan {number}")
            },
            style!(size: 13, color: LABEL, align: TextAlign::Center),
        ));
        if *rpm > 0.0 {
            draws.push(Draw::text(
                cx,
                label_y + 16.0,
                format!("{rpm:.0} RPM"),
                style!(size: 13, weight: FontWeight::SEMIBOLD, color: WHITE, align: TextAlign::Center),
            ));
        }
    }
    canvas(props!(width: width, height: height), draws)
}

fn fan_ring(cx: f32, cy: f32, radius: f32, pct: f64) -> Vec<Draw> {
    let stroke = (radius * 0.16).clamp(5.0, 8.0);
    let mut draws = vec![
        Draw::circle(cx, cy, radius, Color::from_rgba(255, 255, 255, 36)),
        Draw::circle(cx, cy, (radius - stroke).max(8.0), CARD),
    ];
    let fraction = (pct / 100.0).clamp(0.0, 1.0) as f32;
    if fraction > 0.02 {
        draws.push(path!(
            ring_arc(cx, cy, radius - stroke * 0.5, fraction),
            stroke: stroke,
            color: Color::from_rgb(110, 72, 255)
        ));
    }
    let label = if pct > 0.0 { format!("{pct:.0}%") } else { "—".to_owned() };
    let size = (radius * 0.55).clamp(18.0, 28.0) as u32;
    draws.push(Draw::text(
        cx,
        cy - size as f32 * 0.45,
        label,
        style!(size: size, weight: FontWeight::BOLD, color: WHITE, align: TextAlign::Center),
    ));
    draws
}

fn ring_arc(cx: f32, cy: f32, radius: f32, fraction: f32) -> Vec<(f32, f32)> {
    let fraction = fraction.clamp(0.04, 1.0);
    let steps = ((28.0 * fraction).ceil() as i32).max(2);
    let start = -core::f32::consts::FRAC_PI_2;
    let sweep = fraction * core::f32::consts::TAU;
    let mut points = Vec::with_capacity(steps as usize + 1);
    for step in 0..=steps {
        let angle = start + sweep * (step as f32 / steps as f32);
        points.push((cx + angle.cos() * radius, cy + angle.sin() * radius));
    }
    points
}

fn pool_column(miner: &Miner, width: f32, height: f32) -> Node {
    let gap = 8.0;
    let usable = height - gap * 2.0;
    let pool_h = 110.0;
    let extra = if miner.family == "canaan" || (miner.family == "braiins" && !braiins_tunes(&miner.tune).is_empty()) {
        44.0
    } else {
        0.0
    };
    let controls_h = 128.0 + extra;
    let chart_h = (usable - pool_h - controls_h).max(64.0);
    col(
        props!(width: width, height: height, gap: gap),
        [
            pool_card(miner, width, pool_h),
            chart_card(miner, width, chart_h),
            controls_card(miner, width, controls_h),
        ],
    )
}

fn pool_card(miner: &Miner, width: f32, height: f32) -> Node {
    let url = if miner.pool.is_empty() {
        "—".to_owned()
    } else {
        miner.pool.clone()
    };
    let user = if miner.pool_user.is_empty() {
        "—".to_owned()
    } else {
        miner.pool_user.clone()
    };
    let alive = if miner.reachable { "Alive" } else { "Offline" };
    let text_w = (width - 28.0).max(40.0);
    card(
        "Pool Status",
        width,
        height,
        Color::from_hex(0x2A_4A_38),
        vec![
            text(
                alive,
                style!(
                    size: 28,
                    weight: FontWeight::BOLD,
                    color: if miner.reachable { EMERALD } else { RED },
                ),
            ),
            clipped(url, 16, FontWeight::SEMIBOLD, WHITE, text_w),
            clipped(user, 14, FontWeight::REGULAR, MUTED, text_w),
            text(
                format!(
                    "{} accepted    {} rejected",
                    count_text(miner.shares_accepted),
                    count_text(miner.shares_rejected)
                ),
                style!(size: 15, weight: FontWeight::SEMIBOLD, color: WHITE),
            ),
        ],
    )
}

fn chart_card(miner: &Miner, width: f32, height: f32) -> Node {
    let plot_w = (width - 28.0).max(40.0);
    let plot_h = (height - 62.0).max(36.0);
    let now = SystemTime::now().unix_secs;
    let (draws, _) = chart_plot(&miner.chart, now, 15 * 60, plot_w, plot_h, false);
    col(
        props!(
            width: width,
            height: height,
            background: CARD,
            border_radius: 16.0,
            border_width: 1.5,
            border_color: Color::from_hex(0x2A_4A_38),
            padding: 12.0,
            gap: 4.0,
        ),
        [
            text(
                "Charts",
                style!(size: 20, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0),
            ),
            text(
                format!("{}   ·   15 min", display_hashrate(miner)),
                style!(size: 16, color: MUTED, line_height: 1.0),
            ),
            canvas(props!(width: plot_w, height: plot_h), draws),
            touchable(
                "chart-open",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn chart_screen(miner: &Miner, app: &App, width: f32, height: f32) -> Node {
    let height = screen_h(height);
    let pad = 8.0;
    let inner = (width - pad * 2.0).max(100.0);
    let span_index = (app.chart_span as usize).min(CHART_SPANS.len() - 1);
    let (span, _) = CHART_SPANS[span_index];
    let now = SystemTime::now().unix_secs;
    let header_h = 52.0;
    let range_h = 48.0;
    let gap = 4.0;
    let plot_h = (height - pad * 2.0 - header_h - range_h - gap * 2.0).max(120.0);
    let plot_pad = 10.0;
    let (draws, _) = chart_plot(
        &miner.chart,
        now,
        span,
        (inner - plot_pad * 2.0).max(40.0),
        (plot_h - plot_pad * 2.0).max(40.0),
        true,
    );
    let gap_count = 7.0;
    let chip_w = ((inner - 8.0 * gap_count) / CHART_SPANS.len() as f32).max(40.0);
    let chips: Vec<Node> = CHART_SPANS
        .iter()
        .enumerate()
        .map(|(index, (_, name))| range_chip(&format!("span-{index}"), name, index == span_index, chip_w))
        .collect();
    with_theme(
        width,
        height,
        col(
            props!(background: TRANSPARENT, width: width, height: height, padding: pad, gap: gap),
            [
                row(
                    props!(
                        height: header_h,
                        justify_content: Justify::SpaceBetween,
                        cross_align: CrossAlign::Center,
                    ),
                    [
                        row(
                            props!(gap: 10.0, cross_align: CrossAlign::Center),
                            [
                                back_button("back"),
                                clipped(
                                    format!("{}  |  {}", miner_title(miner), miner.ip),
                                    22,
                                    FontWeight::BOLD,
                                    WHITE,
                                    (inner - 420.0).max(80.0),
                                ),
                            ],
                        ),
                        text(
                            display_hashrate(miner),
                            style!(size: 28, weight: FontWeight::BOLD, color: EMERALD, line_height: 1.0),
                        ),
                    ],
                ),
                row(props!(height: range_h, gap: 8.0), chips),
                col(
                    props!(
                        width: inner,
                        height: plot_h,
                        background: Color::from_rgb(64, 64, 64),
                        border_radius: 12.0,
                        padding: plot_pad,
                    ),
                    [canvas(
                        props!(
                            width: (inner - plot_pad * 2.0).max(40.0),
                            height: (plot_h - plot_pad * 2.0).max(40.0),
                        ),
                        draws,
                    )],
                ),
            ],
        ),
    )
}

fn range_chip(id: &str, label: &str, selected: bool, width: f32) -> Node {
    col(
        props!(
            width: width,
            height: 48.0,
            background: if selected { Color::from_hex(0x14_3A_24) } else { Color::from_hex(0x1C_2E_24) },
            border_radius: 10.0,
            border_width: 1.5,
            border_color: if selected { EMERALD } else { Color::from_hex(0x3E_6B_50) },
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                label,
                style!(
                    size: 16,
                    weight: FontWeight::SEMIBOLD,
                    color: if selected { EMERALD } else { WHITE },
                    line_height: 1.0,
                    align: TextAlign::Center,
                ),
            ),
            touchable(
                id,
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn controls_card(miner: &Miner, width: f32, height: f32) -> Node {
    let inner = (width - 28.0).max(80.0);
    let mut rows = vec![row(
        props!(gap: 8.0, width: inner),
        [
            control_art("act-restart", &CONTROL_RESTART),
            control_art("act-pause", &CONTROL_PAUSE),
            control_art("act-resume", &CONTROL_RESUME),
        ],
    )];
    if miner.family == "canaan" {
        let modes: Vec<Node> = canaan_mode_buttons(miner)
            .into_iter()
            .map(|(id, label)| compact_pill(id, label, false))
            .collect();
        rows.push(row(props!(gap: 8.0, width: inner), modes));
    }
    if miner.family == "braiins" {
        let tunes = braiins_tunes(&miner.tune);
        if !tunes.is_empty() {
            let buttons: Vec<Node> = tunes
                .iter()
                .enumerate()
                .map(|(index, tune)| compact_pill(&format!("tune-{index}"), &tune.label, false))
                .collect();
            rows.push(row(props!(gap: 8.0, width: inner, wrap: true), buttons));
        }
    }
    packed_card("Controls", width, height, EMERALD, rows)
}

fn control_art(id: &str, image: &Bitmap) -> Node {
    touchable(
        id,
        props!(width: 88.0, height: 88.0),
        [Draw::bitmap(0.0, 0.0, 88.0, 88.0, image)],
    )
}

fn compact_pill(id: &str, label: &str, selected: bool) -> Node {
    choice_button(id, label, Some(selected), 40.0, 16)
}

fn shows_fan_controls(miner: &Miner) -> bool {
    matches!(miner.family, "bitaxe" | "luckyMiner" | "harlo" | "canaan" | "bitmainZEC")
}

fn packed_card(title: &str, width: f32, height: f32, accent: Color, children: Vec<Node>) -> Node {
    let mut body = vec![text(
        title,
        style!(size: 20, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0),
    )];
    body.extend(children);
    col(
        props!(
            width: width,
            height: height,
            background: CARD,
            border_radius: 16.0,
            border_width: 1.5,
            border_color: accent,
            padding: 10.0,
            gap: 4.0,
            justify_content: Justify::Start,
        ),
        body,
    )
}

fn card(title: &str, width: f32, height: f32, accent: Color, children: Vec<Node>) -> Node {
    let mut body = vec![text(
        title,
        style!(size: 20, weight: FontWeight::SEMIBOLD, color: WHITE, line_height: 1.0),
    )];
    body.extend(children);
    col(
        props!(
            width: width,
            height: height,
            background: CARD,
            border_radius: 16.0,
            border_width: 1.5,
            border_color: accent,
            padding: 12.0,
            gap: 6.0,
            justify_content: Justify::SpaceBetween,
        ),
        body,
    )
}

fn stat_tile(label: &str, value: &str, color: Color, width: f32, height: f32) -> Node {
    let text_w = (width - 12.0).max(24.0) as u32;
    col(
        props!(
            width: width,
            height: height,
            background: Color::from_hex(0x0C_1E_14),
            border_radius: 12.0,
            padding: 6.0,
            gap: 6.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                label,
                style!(
                    size: 20,
                    weight: FontWeight::SEMIBOLD,
                    color: LABEL,
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                    text_overflow: TextOverflow::Ellipsis,
                    max_width: text_w,
                ),
            ),
            text(
                value,
                style!(
                    size: 32,
                    weight: FontWeight::BOLD,
                    family: FontFamily::DeckSans,
                    color: color,
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                    text_overflow: TextOverflow::Ellipsis,
                    max_width: text_w,
                ),
            ),
        ],
    )
}

fn qr_tile(size: f32, image: &Bitmap) -> Node {
    let inner = (size - 16.0).max(40.0);
    col(
        props!(
            width: size,
            height: size,
            background: WHITE,
            border_radius: 8.0,
            padding: 8.0,
        ),
        [canvas(
            props!(width: inner, height: inner),
            [Draw::bitmap(0.0, 0.0, inner, inner, image)],
        )],
    )
}

fn clipped(value: impl Into<String>, size: u32, weight: FontWeight, color: Color, width: f32) -> Node {
    text(
        value.into(),
        style!(
            size: size,
            weight: weight,
            color: color,
            family: FontFamily::DeckSans,
            line_height: 1.1,
            text_overflow: TextOverflow::Ellipsis,
            max_width: width.max(20.0) as u32,
        ),
    )
}

fn format_mhz(value: Option<f64>) -> String {
    match value {
        Some(mhz) if mhz > 0.0 => format!("{mhz:.0} MHz"),
        _ => "—".to_owned(),
    }
}

fn format_mv(value: Option<f64>) -> String {
    match value {
        Some(mv) if mv > 0.0 => format!("{mv:.0} mV"),
        _ => "—".to_owned(),
    }
}

fn choice_button(id: &str, label: &str, selected: Option<bool>, height: f32, size: u32) -> Node {
    let on = selected.unwrap_or(false);
    col(
        props!(
            height: height,
            background: if on { Color::from_hex(0x14_3A_24) } else { Color::from_hex(0x1C_2E_24) },
            border_radius: 8.0,
            border_width: 1.5,
            border_color: if on { EMERALD } else { Color::from_hex(0x3E_6B_50) },
            padding: 8.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                label,
                style!(
                    size: size,
                    weight: FontWeight::SEMIBOLD,
                    color: if on { EMERALD } else { WHITE },
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                ),
            ),
            touchable(
                id,
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn view_edge() -> Node {
    canvas(
        props!(width: 2.0, height: FLEET_HEADER_H),
        [Draw::rect(0.0, 0.0, 2.0, FLEET_HEADER_H, Color::from_rgb(88, 88, 88))],
    )
}

fn view_segment(id: &str, label: &str, selected: bool) -> Node {
    col(
        props!(
            width: 132.0,
            height: FLEET_HEADER_H,
            background: if selected { EMERALD } else { TRANSPARENT },
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                label,
                style!(
                    size: 24,
                    weight: FontWeight::BOLD,
                    color: if selected { Color::from_rgb(8, 16, 12) } else { WHITE },
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                ),
            ),
            touchable(
                id,
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn led_effect_name(effect: LedEffect) -> &'static str {
    match effect {
        LedEffect::Solid => "Solid",
        LedEffect::Breathe => "Breathe",
        LedEffect::Chase => "Chase",
        LedEffect::Scan => "Scan",
        LedEffect::Snake => "Snake",
        LedEffect::KnightRider => "Night Rider",
    }
}

fn settings_mode_pill(id: &str, label: &str, selected: bool) -> Node {
    choice_button(id, label, Some(selected), 52.0, 22)
}

fn pill(id: &str, label: &str) -> Node {
    choice_button(id, label, None, 44.0, 18)
}

fn settings_pill(id: &str, label: &str) -> Node {
    choice_button(id, label, None, 52.0, 22)
}

fn status_label(miner: &Miner) -> &'static str {
    if !miner.reachable {
        "Offline"
    } else if miner.hashrate_ths.unwrap_or(0.0) <= 0.0 {
        "Idle"
    } else {
        "Online"
    }
}

fn format_difficulty(value: Option<f64>) -> String {
    match value {
        Some(difficulty) if difficulty >= 1_000_000_000_000.0 => format!("{:.2}T", difficulty / 1_000_000_000_000.0),
        Some(difficulty) if difficulty >= 1_000_000_000.0 => format!("{:.2}G", difficulty / 1_000_000_000.0),
        Some(difficulty) if difficulty >= 1_000_000.0 => format!("{:.2}M", difficulty / 1_000_000.0),
        Some(difficulty) if difficulty >= 1_000.0 => format!("{:.1}K", difficulty / 1_000.0),
        Some(difficulty) if difficulty > 0.0 => format!("{difficulty:.0}"),
        _ => "—".to_owned(),
    }
}

fn format_fan(fan: Option<f64>) -> String {
    match fan {
        Some(value) if value > 100.0 => format!("{:.0} rpm", value),
        Some(value) if value > 0.0 => format!("{:.0}%", value),
        _ => "—".to_owned(),
    }
}

fn progress_line(app: &App) -> String {
    match app.phase {
        Phase::NeedNetwork => app.message.clone(),
        Phase::Http | Phase::Tcp => format!("{}   {}/{}", app.message, app.finished, app.total),
        Phase::Live => {
            let online = app.miners.iter().filter(|miner| miner.reachable).count();
            format!("{online} online")
        }
    }
}

fn deck_night() -> bool {
    bmc_wasm_sdk::system::current().night_mode() == Some(true)
}

fn snap_brightness(frac: f32) -> u8 {
    let pct = 10.0 + frac.clamp(0.0, 1.0) * 90.0;
    let stepped = (pct / 5.0).round() * 5.0;
    stepped.clamp(10.0, 100.0) as u8
}

fn brightness_frac(pct: u8) -> f32 {
    if pct < 10 {
        0.5
    } else {
        ((f32::from(pct) - 10.0) / 90.0).clamp(0.0, 1.0)
    }
}

fn shown_brightness(app: &App) -> u8 {
    if deck_night() { app.night_brightness } else { app.brightness }
}

fn screen_off_caption(app: &App) -> String {
    if !app.screen_off_known {
        return "Deck screen off: reading".to_owned();
    }
    match app.screen_off_secs.filter(|secs| *secs > 0) {
        None => "Deck screen off is off".to_owned(),
        Some(secs) if secs < 60 => format!("Night mode dims after {secs}s"),
        Some(secs) => format!("Night mode dims after {}m", secs / 60),
    }
}

fn alarm_request(app: &App) -> Vec<u8> {
    let hour24 = match (app.alarm_hour, app.alarm_pm) {
        (12, false) => 0,
        (12, true) => 12,
        (hour, true) => hour.saturating_add(12),
        (hour, false) => hour,
    };
    let time = format!("{hour24:02}:{:02}", app.alarm_minute);
    let sound = ALARM_SOUNDS
        .get(app.alarm_sound as usize)
        .map(|item| item.0)
        .unwrap_or(ALARM_SOUNDS[0].0);
    let mut msg = proto_string_field(1, app.alarm_name.trim());
    msg.extend(proto_string_field(2, &time));
    msg.extend(proto_varint_field(3, 1));
    for day in 0u8..7 {
        if app.alarm_days & (1 << day) != 0 {
            msg.extend(proto_varint_field(4, u64::from(day) + 1));
        }
    }
    msg.extend(proto_string_field(5, sound));
    let snooze = if app.alarm_snooze {
        let mut options = proto_varint_field(1, u64::from(app.alarm_limit.max(1)));
        options.extend(proto_varint_field(2, 1));
        proto_bytes_field(1, &options)
    } else {
        proto_bytes_field(2, &[])
    };
    msg.extend(proto_bytes_field(6, &snooze));
    msg
}

fn proto_varint_bytes(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

fn proto_varint_field(field: u32, value: u64) -> Vec<u8> {
    let mut out = proto_varint_bytes(u64::from(field << 3));
    out.extend(proto_varint_bytes(value));
    out
}

fn proto_string_field(field: u32, text: &str) -> Vec<u8> {
    proto_bytes_field(field, text.as_bytes())
}

fn proto_bytes_field(field: u32, bytes: &[u8]) -> Vec<u8> {
    let mut out = proto_varint_bytes(u64::from((field << 3) | 2));
    out.extend(proto_varint_bytes(bytes.len() as u64));
    out.extend_from_slice(bytes);
    out
}

fn led_color_on(app: &App, rgb: (u8, u8, u8)) -> bool {
    app.led_hold && !app.led_rainbow && app.led_rgb == rgb
}

fn payload_value(raw: &str, key: &str) -> String {
    raw.split(';').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name == key).then(|| value.trim().to_owned())
    }).unwrap_or_default()
}

fn inventory_param_stamp() -> String {
    let snap = bmc_wasm_sdk::params::current();
    ["inventory", "inventory2", "inventory3", "inventory4"]
        .into_iter()
        .map(|key| snap.get_str(key).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_hashwatcher_widget(msg: &[u8]) -> Option<(String, String)> {
    const UID: &str = "c4e8a1d2-7b63-4f0e-9a15-6d2b8f0c3e71";
    for scene in proto_len_all(msg, 1) {
        let Some(scene_id) = proto_string(&scene, 1) else {
            continue;
        };
        for kind in [4u32, 5] {
            for wrapper in proto_len_all(&scene, kind) {
                for widget in proto_len_all(&wrapper, 1) {
                    let Some(widget_id) = proto_string(&widget, 1) else {
                        continue;
                    };
                    let Some(config) = proto_bytes(&widget, 4) else {
                        continue;
                    };
                    if proto_string(config, 1).as_deref() == Some(UID) && !scene_id.is_empty() && !widget_id.is_empty() {
                        return Some((scene_id, widget_id));
                    }
                }
            }
        }
    }
    None
}

fn widget_update(id: &str, scene: &str, deck: &str) -> Vec<u8> {
    let snap = bmc_wasm_sdk::params::current();
    let mut params = Vec::new();
    for key in ["inventory", "inventory2", "inventory3", "inventory4"] {
        params.extend(widget_param(key, snap.get_str(key).unwrap_or("")));
    }
    params.extend(widget_param("deck", deck));
    let mut msg = proto_string_field(1, id);
    msg.extend(proto_string_field(2, scene));
    msg.extend(proto_varint_field(4, 4));
    msg.extend(proto_bytes_field(5, &params));
    msg
}

fn widget_param(key: &str, value: &str) -> Vec<u8> {
    let value = proto_string_field(5, value);
    let mut entry = proto_string_field(1, key);
    entry.extend(proto_bytes_field(2, &value));
    proto_bytes_field(1, &entry)
}

fn proto_len_all(msg: &[u8], field: u32) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < msg.len() {
        let Some(tag) = read_varint(msg, &mut index) else {
            break;
        };
        let number = (tag >> 3) as u32;
        match tag & 7 {
            0 => {
                if read_varint(msg, &mut index).is_none() {
                    break;
                }
            }
            2 => {
                let Some(len) = read_varint(msg, &mut index) else {
                    break;
                };
                let len = len as usize;
                if index + len > msg.len() {
                    break;
                }
                if number == field {
                    out.push(msg[index..index + len].to_vec());
                }
                index += len;
            }
            5 => index += 4,
            1 => index += 8,
            _ => break,
        }
    }
    out
}

fn grpc_frame(msg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + msg.len());
    out.push(0);
    out.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    out.extend_from_slice(msg);
    out
}

fn grpc_message(body: &[u8]) -> &[u8] {
    let mut index = 0;
    let mut message: &[u8] = &[];
    while index + 5 <= body.len() {
        let flag = body[index];
        let len = u32::from_be_bytes([
            body[index + 1],
            body[index + 2],
            body[index + 3],
            body[index + 4],
        ]) as usize;
        let end = index + 5 + len;
        if end > body.len() {
            break;
        }
        if flag & 0x80 == 0 {
            message = &body[index + 5..end];
        }
        index = end;
    }
    message
}

fn read_varint(data: &[u8], index: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    while *index < data.len() && shift < 64 {
        let byte = data[*index];
        *index += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn proto_varint(msg: &[u8], field: u32) -> Option<u64> {
    let mut index = 0;
    while index < msg.len() {
        let tag = read_varint(msg, &mut index)?;
        let number = (tag >> 3) as u32;
        match tag & 7 {
            0 => {
                let value = read_varint(msg, &mut index)?;
                if number == field {
                    return Some(value);
                }
            }
            2 => {
                let len = read_varint(msg, &mut index)? as usize;
                if index + len > msg.len() {
                    return None;
                }
                index += len;
            }
            5 => index += 4,
            1 => index += 8,
            _ => return None,
        }
    }
    None
}

fn proto_bytes<'a>(msg: &'a [u8], field: u32) -> Option<&'a [u8]> {
    let mut index = 0;
    while index < msg.len() {
        let tag = read_varint(msg, &mut index)?;
        let number = (tag >> 3) as u32;
        match tag & 7 {
            0 => {
                read_varint(msg, &mut index)?;
            }
            2 => {
                let len = read_varint(msg, &mut index)? as usize;
                if index + len > msg.len() {
                    return None;
                }
                if number == field {
                    return Some(&msg[index..index + len]);
                }
                index += len;
            }
            5 => index += 4,
            1 => index += 8,
            _ => return None,
        }
    }
    None
}

fn proto_string(msg: &[u8], field: u32) -> Option<String> {
    let bytes = proto_bytes(msg, field)?;
    String::from_utf8(bytes.to_vec()).ok()
}

fn uint32_value(value: u32) -> Vec<u8> {
    let mut msg = vec![0x08];
    let mut rest = value;
    loop {
        let mut byte = (rest & 0x7f) as u8;
        rest >>= 7;
        if rest != 0 {
            byte |= 0x80;
        }
        msg.push(byte);
        if rest == 0 {
            break;
        }
    }
    msg
}

fn handle_brightness(drags: &HashMap<String, TouchHit>) {
    let mut changed = false;
    APP.with(|slot| {
        let mut app = slot.borrow_mut();
        let Some(hit) = drags.get("brightness") else {
            if app.brightness_drag {
                app.brightness_drag = false;
                app.push_brightness();
                app.touch_controls();
            }
            return;
        };
        app.brightness_drag = true;
        let pct = snap_brightness(hit.frac_x());
        let night = deck_night();
        let current = if night { app.night_brightness } else { app.brightness };
        if pct != current {
            if night {
                app.night_brightness = pct;
            } else {
                app.brightness = pct;
            }
            changed = true;
            app.push_brightness();
        }
    });
    if changed {
        request_frame();
    }
}

fn handle_clicks(clicks: &HashMap<String, TouchHit>) {
    let swallow = APP.with(|slot| {
        let mut app = slot.borrow_mut();
        let swallow = app.swallow_touch;
        app.swallow_touch = false;
        swallow
    });
    if swallow {
        return;
    }
    let mut changed = false;
    APP.with(|slot| {
        let mut app = slot.borrow_mut();
        for id in clicks.keys() {
            if id == "fleet-hit" || id == "neural-hit" || id == "brightness" {
                continue;
            }
            changed |= app.handle_click(id);
        }
    });
    if changed {
        let paused = APP.with(|app| app.borrow().settings);
        if !paused {
            APP.with(|app| app.borrow_mut().fill());
        }
        request_frame();
    }
}

fn handle_fleet_gesture(clicks: &HashMap<String, TouchHit>, drags: &HashMap<String, TouchHit>, width: f32, height: f32) {
    let mut changed = false;
    APP.with(|slot| {
        let mut app = slot.borrow_mut();
        if app.selected.is_some() {
            app.gesture_y = None;
            app.gesture_moved = false;
            return;
        }
        if let Some(hit) = clicks.get("neural-hit") {
            let (canvas_w, _) = neural_canvas_size(width, height);
            if neural_exit_hit(hit.x, hit.y, canvas_w) {
                app.neural = false;
                app.summary = false;
                app.neural_hint_ms = 0;
                app.idle_ms = 0;
                changed = true;
            } else if neural_center_hit(hit.x, hit.y, width, height) {
                app.label_mode = match app.label_mode {
                    0 => 1,
                    1 => 2,
                    _ => 0,
                };
                app.store_saver();
                changed = true;
            } else if let Some(ip) = neural_ip_at(&app, hit.x, hit.y, width, height, app.neural_spin_drawn) {
                app.selected = Some(ip);
                app.notice.clear();
                changed = true;
            } else {
                app.neural_hint_ms = 5_450;
                changed = true;
            }
        }
        if let Some(hit) = drags.get("fleet-hit") {
            match app.gesture_y {
                Some(previous) => {
                    let delta = hit.y - previous;
                    if delta.abs() > 12.0 {
                        app.gesture_moved = true;
                    }
                    if delta <= -48.0 {
                        app.fleet_row = app.fleet_row.saturating_add(1);
                        app.gesture_y = Some(hit.y);
                        changed = true;
                    } else if delta >= 48.0 {
                        app.fleet_row = app.fleet_row.saturating_sub(1);
                        app.gesture_y = Some(hit.y);
                        changed = true;
                    }
                }
                None => app.gesture_y = Some(hit.y),
            }
            let max_start = fleet_max_band(&fleet_rows(&app), fleet_list_height(height));
            if app.fleet_row > max_start {
                app.fleet_row = max_start;
            }
        } else {
            app.gesture_y = None;
        }
        if let Some(hit) = clicks.get("fleet-hit") {
            let moved = app.gesture_moved;
            app.gesture_moved = false;
            if !moved {
                if let Some(ip) = fleet_ip_at(&app, hit.x, hit.y, width, height) {
                    app.selected = Some(ip);
                    app.notice.clear();
                    changed = true;
                }
            }
        } else if drags.get("fleet-hit").is_none() {
            app.gesture_moved = false;
        }
    });
    if changed {
        request_frame();
    }
}

fn neural_center_hit(x: f32, y: f32, width: f32, height: f32) -> bool {
    let (canvas_w, canvas_h) = neural_canvas_size(width, height);
    let cx = canvas_w * 0.5;
    let cy = canvas_h * 0.50;
    let nucleus = (canvas_w.min(canvas_h) * 0.30).clamp(96.0, 150.0);
    (x - cx).hypot(y - cy) <= nucleus * 0.42
}

fn neural_ip_at(app: &App, x: f32, y: f32, width: f32, height: f32, spin: f32) -> Option<String> {
    let (plot_w, plot_h) = neural_canvas_size(width, height);
    let points = neural_points(app.miners.len(), plot_w, plot_h, spin);
    let mut best: Option<(usize, f32)> = None;
    for (index, (px, py)) in points.iter().copied().enumerate() {
        let distance = (px - x).hypot(py - y);
        if distance < 48.0 && best.as_ref().is_none_or(|(_, best_distance)| distance < *best_distance) {
            best = Some((index, distance));
        }
    }
    best.and_then(|(index, _)| app.miners.get(index).map(|miner| miner.ip.clone()))
}

fn fleet_ip_at(app: &App, x: f32, y: f32, width: f32, height: f32) -> Option<String> {
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let list_h = fleet_list_height(height);
    let bands = fleet_rows(app);
    let max_start = fleet_max_band(&bands, list_h);
    let start = if max_start > 0 {
        app.fleet_row.min(max_start)
    } else {
        0
    };
    let card_w = fleet_card_width(width);
    let stride_x = card_w + FLEET_GAP;
    let mut cursor = 0.0;
    for band in bands.iter().skip(start).take(fleet_fit_count(&bands, start, list_h)) {
        let band_h = band_height(band);
        if y >= cursor && y < cursor + band_h {
            let FleetBand::Cards(indexes) = band else {
                return None;
            };
            let column = (x / stride_x) as usize;
            let local_x = x - column as f32 * stride_x;
            if column >= indexes.len() || local_x > card_w {
                return None;
            }
            return app.miners.get(indexes[column]).map(|miner| miner.ip.clone());
        }
        cursor += band_h + FLEET_GAP;
    }
    None
}

#[unsafe(no_mangle)]
pub extern "C" fn init() {
    APP.with(|app| app.borrow_mut().begin());
    APP.with(|app| app.borrow_mut().fill());
    request_frame();
}

#[unsafe(no_mangle)]
pub extern "C" fn on_touch() {
    APP.with(|slot| {
        let mut app = slot.borrow_mut();
        if app.saver_on {
            app.swallow_touch = true;
        }
        app.saver_on = false;
        app.idle_ms = 0;
    });
    request_frame();
}

#[unsafe(no_mangle)]
pub extern "C" fn on_network_update() {
    let needed = APP.with(|app| app.borrow().phase == Phase::NeedNetwork);
    if needed {
        APP.with(|app| app.borrow_mut().begin());
        APP.with(|app| app.borrow_mut().fill());
    }
    request_frame();
}

#[unsafe(no_mangle)]
pub extern "C" fn render(delta_ms: u32) {
    TICK.set(TICK.get().saturating_add(1));
    let scanning = APP.with(|slot| {
        let mut app = slot.borrow_mut();
        // Settings keeps the LEDs running and ignores miner polls, scans,
        // weather, and share animations so a scroll is not rebuilding that work.
        app.flush_charts(false);
        let paused = app.settings;
        if !paused {
            app.maintain_hash_order(delta_ms);
            app.reap_sockets();
            // A delayed frame is the poll clock. Touch and fetch frames have a small delta,
            // so they must not start another poll round.
            if app.share_flash_ms > 0 {
                app.share_flash_ms = app.share_flash_ms.saturating_sub(delta_ms);
            }
            if app.phase == Phase::Live
                && app.queue.is_empty()
                && app.inflight.is_empty()
                && app.sockets.is_empty()
                && !app.miners.is_empty()
            {
                app.since_poll = app.since_poll.saturating_add(delta_ms);
                if app.since_poll >= app.poll_ms {
                    app.since_poll = 0;
                    app.enqueue_polls();
                }
            }
            app.promote_best();
            app.poll_deck(delta_ms);
            app.maybe_fetch_weather();
            app.fill();
        } else if app.tailscale_open
            || !app.tailscale_hold.is_empty()
            || app.tailscale_state.is_empty()
        {
            app.poll_tailscale(delta_ms);
        }
        app.drive_celebration_leds(delta_ms);
        app.drive_user_leds(delta_ms);
        if app.celebration.is_some() {
            app.idle_ms = 0;
            app.saver_on = false;
        } else if app.swallow_touch {
            app.idle_ms = 0;
            app.saver_on = false;
        } else {
            app.idle_ms = app.idle_ms.saturating_add(delta_ms.min(2_000));
            let deck_ms = if deck_night() {
                app.screen_off_secs
                    .filter(|secs| *secs > 0)
                    .map(|secs| secs.saturating_mul(1_000))
                    .unwrap_or(0)
            } else {
                0
            };
            let mut limit = app.saver_ms;
            if deck_ms > 0 && (limit == 0 || deck_ms < limit) {
                limit = deck_ms;
            }
            if limit > 0 && !app.settings && app.idle_ms >= limit {
                app.saver_on = true;
            }
        }
        app.maybe_check_update(delta_ms);
        matches!(app.phase, Phase::Http | Phase::Tcp)
    });

    let size = widget_size();
    let width = size.width as f32;
    let height = size.height as f32;
    let root = APP.with(|slot| {
        let app = slot.borrow();
        if let Some(popup) = app.celebration.as_ref() {
            return best_popup(popup, width, height);
        }
        if app.saver_on && !app.settings {
            return screensaver_view(&app, width, height);
        }
        if app.settings {
            return settings_view(&app, width, height);
        }
        if let Some(ip) = app.selected.clone() {
            if let Some(miner) = app.miners.iter().find(|miner| miner.ip == ip).cloned() {
                if app.chart_open {
                    return chart_screen(&miner, &app, width, height);
                }
                return dashboard(&miner, &app, width, height);
            }
        }
        fleet_view(&app, width, height)
    });
    let drawn = render_ui(size.width, size.height, root);
    let settings_open = APP.with(|app| app.borrow().settings);
    if !settings_open {
        handle_fleet_gesture(&drawn.clicks, &drawn.drags, width, height);
    }
    handle_brightness(&drawn.drags);
    handle_clicks(&drawn.clicks);
    let (animate_neural, flash_left, celebrating, settings) = APP.with(|app| {
        let app = app.borrow();
        (
            app.neural && !app.saver_on && app.selected.is_none() && !app.settings && app.celebration.is_none() && !app.miners.is_empty(),
            app.share_flash_ms,
            app.celebration.is_some(),
            app.settings && app.celebration.is_none(),
        )
    });
    if settings {
        // LED effects re-arm on this clock. Miner animation frames stay off.
        request_frame_after(1_000);
    } else if animate_neural {
        APP.with(|slot| {
            let mut app = slot.borrow_mut();
            app.neural_hint_ms = app.neural_hint_ms.saturating_sub(delta_ms);
            app.neural_spin_drawn = app.neural_spin;
            let step = (delta_ms.min(50) as f32) * (0.0034 / 16.0);
            app.neural_spin += step;
        });
        request_frame();
    } else if scanning {
        request_frame_after(FRAME_MS);
    } else if celebrating {
        request_frame_after(80);
    } else if flash_left > 0 {
        request_frame_after(16);
    } else {
        request_frame_after(1000);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn on_params_update() {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let stamp = inventory_param_stamp();
        if app.inventory_seen && stamp == app.inventory_stamp {
            app.apply_deck_controls();
        } else {
            app.apply_inventory();
        }
    });
    let paused = APP.with(|app| app.borrow().settings);
    if !paused {
        APP.with(|app| app.borrow_mut().fill());
    }
    request_frame();
}
