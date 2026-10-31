//! EcoFlow power monitor. Credentials arrive from HashWatcher as an inventory
//! line (`#power-monitor` plus JSON) or as `pm=<base64 json>` on the deck
//! control string. Live watts come from EcoFlow's MQTT feed.

use std::cell::Cell;
use std::time::Duration;

use bmc_wasm_sdk::socket::{self, Socket, SocketEvent};
use bmc_wasm_sdk::*;

const MARKER: &str = "#power-monitor";
const COARSE_KEEP_SECS: i64 = 48 * 3600;
/// Exact refresh-rate samples kept per series. 12_000 is three hours at 1s
/// and the full 48h at 30s or slower.
const MAX_FINE_POINTS: usize = 12_000;
/// Points the chart stroke actually draws. The ring stays full in memory.
const DRAW_POINTS: usize = 240;
/// Fine samples written to the Deck's flash cache. The live chart keeps the
/// full ring in memory. Flash only needs a short tail; older history is the
/// minute averages, so a save stays a small overwrite.
const PERSIST_FINE_MAX: usize = 600;
/// How often the chart file is replaced. A power cut loses at most this much
/// of the newest tail, and the UI thread is not rewriting flash every sample.
const HISTORY_SAVE_SECS: i64 = 10 * 60;
/// Space between `$x.xx today` and `$x.xx/day`.
const COST_GAP: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Series {
    Total,
    L1,
    L2,
    L3,
}

impl Series {
    fn slot(self) -> usize {
        match self {
            Self::Total => 0,
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Span {
    M5,
    M15,
    M30,
    H1,
    H3,
    H5,
    H12,
    H24,
    H48,
}

impl Span {
    fn secs(self) -> i64 {
        match self {
            Self::M5 => 5 * 60,
            Self::M15 => 15 * 60,
            Self::M30 => 30 * 60,
            Self::H1 => 3600,
            Self::H3 => 3 * 3600,
            Self::H5 => 5 * 3600,
            Self::H12 => 12 * 3600,
            Self::H24 => 24 * 3600,
            Self::H48 => 48 * 3600,
        }
    }

    fn key(self) -> u8 {
        match self {
            Self::M5 => 0,
            Self::M15 => 1,
            Self::M30 => 2,
            Self::H1 => 3,
            Self::H3 => 4,
            Self::H5 => 5,
            Self::H12 => 6,
            Self::H24 => 7,
            Self::H48 => 8,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::M5 => "5m",
            Self::M15 => "15m",
            Self::M30 => "30m",
            Self::H1 => "1h",
            Self::H3 => "3h",
            Self::H5 => "5h",
            Self::H12 => "12h",
            Self::H24 => "24h",
            Self::H48 => "48h",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::M5 => "power-m5",
            Self::M15 => "power-m15",
            Self::M30 => "power-m30",
            Self::H1 => "power-h1",
            Self::H3 => "power-h3",
            Self::H5 => "power-h5",
            Self::H12 => "power-h12",
            Self::H24 => "power-h24",
            Self::H48 => "power-h48",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "power-m5" => Self::M5,
            "power-m15" => Self::M15,
            "power-m30" => Self::M30,
            "power-h1" => Self::H1,
            "power-h3" => Self::H3,
            "power-h5" => Self::H5,
            "power-h12" => Self::H12,
            "power-h24" => Self::H24,
            "power-h48" => Self::H48,
            _ => return None,
        })
    }
}

const SPANS: [Span; 9] = [
    Span::M5,
    Span::M15,
    Span::M30,
    Span::H1,
    Span::H3,
    Span::H5,
    Span::H12,
    Span::H24,
    Span::H48,
];

#[derive(Clone)]
struct Reading {
    total_w: Option<f64>,
    l1_w: Option<f64>,
    l2_w: Option<f64>,
    l3_w: Option<f64>,
    l1_v: Option<f64>,
    l2_v: Option<f64>,
    l3_v: Option<f64>,
    l1_a: Option<f64>,
    l2_a: Option<f64>,
    l3_a: Option<f64>,
    grid: String,
}

impl Reading {
    fn blank() -> Self {
        Self {
            total_w: None,
            l1_w: None,
            l2_w: None,
            l3_w: None,
            l1_v: None,
            l2_v: None,
            l3_v: None,
            l1_a: None,
            l2_a: None,
            l3_a: None,
            grid: String::new(),
        }
    }
}

struct Ring {
    fine: Vec<(i64, f32)>,
    coarse: Vec<(i64, f32)>,
    minute_sum: f32,
    minute_n: u16,
    minute_at: i64,
}

impl Ring {
    fn new() -> Self {
        Self {
            fine: Vec::new(),
            coarse: Vec::new(),
            minute_sum: 0.0,
            minute_n: 0,
            minute_at: 0,
        }
    }

    fn clear(&mut self) {
        *self = Self::new();
    }

    fn push(&mut self, now: i64, watts: f32, interval: i64) {
        self.fine.push((now, watts));
        let fine_cut = now - fine_keep_secs(interval);
        if self.fine.first().is_some_and(|(stamp, _)| *stamp < fine_cut) || self.fine.len() > MAX_FINE_POINTS {
            let mut keep = self
                .fine
                .iter()
                .position(|(stamp, _)| *stamp >= fine_cut)
                .unwrap_or(self.fine.len());
            if self.fine.len().saturating_sub(keep) > MAX_FINE_POINTS {
                keep = self.fine.len() - MAX_FINE_POINTS;
            }
            if keep > 0 {
                self.fine.drain(0..keep);
            }
        }
        let minute = now.div_euclid(60);
        if self.minute_n == 0 {
            self.minute_at = minute;
        }
        if minute != self.minute_at && self.minute_n > 0 {
            let avg = self.minute_sum / f32::from(self.minute_n);
            self.coarse.push((self.minute_at * 60, avg));
            let coarse_cut = now - COARSE_KEEP_SECS;
            if self.coarse.first().is_some_and(|(stamp, _)| *stamp < coarse_cut) {
                let keep = self
                    .coarse
                    .iter()
                    .position(|(stamp, _)| *stamp >= coarse_cut)
                    .unwrap_or(self.coarse.len());
                if keep > 0 {
                    self.coarse.drain(0..keep);
                }
            }
            self.minute_sum = 0.0;
            self.minute_n = 0;
            self.minute_at = minute;
        }
        self.minute_sum += watts;
        self.minute_n = self.minute_n.saturating_add(1);
    }
}

pub struct PowerFeed {
    access_key: String,
    secret_key: String,
    region: String,
    serial: String,
    name: String,
    pm: String,
    stamp: String,
    status: String,
    fault: String,
    last_ok_sec: i64,
    grid: String,
    total_w: Option<f64>,
    l1_w: Option<f64>,
    l2_w: Option<f64>,
    l3_w: Option<f64>,
    l1_v: Option<f64>,
    l2_v: Option<f64>,
    l3_v: Option<f64>,
    l1_a: Option<f64>,
    l2_a: Option<f64>,
    l3_a: Option<f64>,
    latest: Option<Reading>,
    rings: [Ring; 4],
    /// Fleet hashrate, sampled on the miner refresh interval.
    hash_ring: Ring,
    hash_on: bool,
    hash_sample: Option<f32>,
    hash_every: u16,
    last_hash_sec: i64,
    /// Vertical ranges already drawn. A new sample inside the range must not
    /// move points that are already on the line. The range only grows, and it
    /// resets when the span or the watt series changes.
    watt_axis: Cell<(u8, f64, f64)>,
    hash_axis: Cell<(u8, f64, f64)>,
    series: Series,
    span: Span,
    rate_text: String,
    rate: f64,
    poll_secs: u16,
    today_kwh: f64,
    day: i32,
    saved_cost: f64,
    saved_day: i32,
    cost_edit: bool,
    /// `$x.xx/day` beside the full-chart watt total. A tap hides it; a tap on
    /// the same spot shows it again.
    cost_on: bool,
    chart_full: bool,
    chrome_on: bool,
    chrome_ms: u32,
    ledger_loaded: bool,
    persist: bool,
    last_sample_sec: i64,
    last_persist_sec: i64,
    socket: Option<Socket>,
    buf: Vec<u8>,
    mqtt_user: String,
    mqtt_pass: String,
    mqtt_host: String,
    topic: String,
    fetch: Option<u32>,
    quota_fetch: Option<u32>,
    quota_ms: u32,
    history_loaded: bool,
    last_history_save: i64,
    region_try: u8,
    connected: bool,
    subscribed: bool,
    ping_ms: u32,
    retry_ms: u32,
    /// Pause after EcoFlow refuses the login. A new client id on every retry
    /// burns the 10 links EcoFlow allows per day, so this waits on the same id.
    hold_ms: u32,
    packet_id: u16,
}

impl PowerFeed {
    pub fn new() -> Self {
        Self {
            access_key: String::new(),
            secret_key: String::new(),
            region: String::new(),
            serial: String::new(),
            name: String::new(),
            pm: String::new(),
            stamp: String::new(),
            status: String::new(),
            fault: String::new(),
            last_ok_sec: 0,
            grid: String::new(),
            total_w: None,
            l1_w: None,
            l2_w: None,
            l3_w: None,
            l1_v: None,
            l2_v: None,
            l3_v: None,
            l1_a: None,
            l2_a: None,
            l3_a: None,
            latest: None,
            rings: [Ring::new(), Ring::new(), Ring::new(), Ring::new()],
            hash_ring: Ring::new(),
            hash_on: true,
            hash_sample: None,
            hash_every: 8,
            last_hash_sec: 0,
            watt_axis: Cell::new((255, 0.0, 0.0)),
            hash_axis: Cell::new((255, 0.0, 0.0)),
            series: Series::Total,
            span: Span::M15,
            rate_text: String::new(),
            rate: 0.0,
            poll_secs: 1,
            today_kwh: 0.0,
            day: 0,
            saved_cost: 0.0,
            saved_day: 0,
            cost_edit: false,
            cost_on: true,
            chart_full: false,
            chrome_on: false,
            chrome_ms: 0,
            ledger_loaded: false,
            persist: false,
            last_sample_sec: 0,
            last_persist_sec: 0,
            socket: None,
            buf: Vec::new(),
            mqtt_user: String::new(),
            mqtt_pass: String::new(),
            mqtt_host: String::new(),
            topic: String::new(),
            fetch: None,
            quota_fetch: None,
            quota_ms: 0,
            history_loaded: false,
            last_history_save: 0,
            region_try: 0,
            connected: false,
            subscribed: false,
            ping_ms: 0,
            retry_ms: 0,
            hold_ms: 0,
            packet_id: 1,
        }
    }

    pub fn reload(&mut self) {
        let snap = params::current();
        let kv_controls = kv::get_string("controls").unwrap_or_default();
        if !kv_controls.trim().is_empty() {
            self.load_ledger(&kv_controls);
        } else if let Some(deck) = snap.get_str("deck") {
            self.load_ledger(deck);
        }
        let mut json = String::new();
        if let Some(saved) = kv::get_string("inventory") {
            json = monitor_json(&saved);
        }
        if json.is_empty() {
            for key in ["inventory", "inventory2", "inventory3", "inventory4"] {
                let Some(chunk) = snap.get_str(key) else {
                    continue;
                };
                json = monitor_json(chunk);
                if !json.is_empty() {
                    break;
                }
            }
        }
        if json.is_empty() && !kv_controls.is_empty() {
            if let Some(encoded) = payload_value(&kv_controls, "pm") {
                if let Some(decoded) = b64_decode(&encoded) {
                    json = String::from_utf8_lossy(&decoded).into_owned();
                }
            }
        }
        if json.is_empty() {
            if let Some(deck) = snap.get_str("deck") {
                if let Some(encoded) = payload_value(deck, "pm") {
                    if let Some(decoded) = b64_decode(&encoded) {
                        json = String::from_utf8_lossy(&decoded).into_owned();
                    }
                }
            }
        }
        let doc = JsonDoc::parse(json.as_bytes());
        let access = doc.str("/accessKey").unwrap_or_default();
        let secret = doc.str("/secretKey").unwrap_or_default();
        let serial = doc.str("/serialNumber").unwrap_or_default();
        let region = doc.str("/region").unwrap_or_default();
        let name = doc.str("/deviceName").unwrap_or_default();
        let stamp = format!("{access}|{serial}|{region}");
        let replacing_device = !self.stamp.is_empty() && stamp != self.stamp;
        if stamp == self.stamp && !access.is_empty() {
            if self.pm.is_empty() && !json.is_empty() {
                self.pm = b64_encode(json.as_bytes());
            }
            return;
        }
        self.stop();
        self.access_key = access;
        self.secret_key = secret;
        self.serial = serial;
        self.region = region;
        self.name = name;
        self.stamp = stamp;
        self.pm = if json.is_empty() { String::new() } else { b64_encode(json.as_bytes()) };
        self.mqtt_user.clear();
        self.mqtt_pass.clear();
        self.mqtt_host.clear();
        self.topic.clear();
        self.hold_ms = 0;
        self.region_try = 0;
        self.total_w = None;
        self.latest = None;
        self.hash_sample = None;
        self.last_hash_sec = 0;
        for ring in &mut self.rings {
            ring.clear();
        }
        self.hash_ring.clear();
        if replacing_device {
            bmc_wasm_sdk::kv::delete("power-chart");
            bmc_wasm_sdk::cache::evict("power-chart");
            self.history_loaded = true;
        }
        self.last_sample_sec = 0;
        self.status = if self.ready() {
            "Connecting".to_owned()
        } else {
            String::new()
        };
    }

    /// Deck control fields. `pm` keeps the EcoFlow login. The rest is the
    /// $/kWh rate and today's energy, so a restart does not lose the day.
    pub fn control_fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        if !self.pm.is_empty() {
            fields.push(format!("pm={}", self.pm));
        }
        if !self.rate_text.is_empty() {
            fields.push(format!("rate={}", self.rate_text));
        }
        fields.push(format!("poll={}", self.poll_secs));
        if self.day != 0 {
            fields.push(format!("kwh={:.4}", self.today_kwh));
            fields.push(format!("day={}", self.day));
        }
        if self.saved_day != 0 {
            fields.push(format!("saved={:.4}", self.saved_cost));
            fields.push(format!("sday={}", self.saved_day));
        }
        fields.push(format!("hov={}", if self.hash_on { "1" } else { "0" }));
        fields.push(format!("cst={}", if self.cost_on { "1" } else { "0" }));
        fields
    }

    pub fn is_chart_full(&self) -> bool {
        self.chart_full
    }

    pub fn full_chart(&self, width: f32, height: f32, fleet_hash: &[(i64, f32)]) -> Node {
        let points = self.window_points();
        let hash_points = self.overlay_hash(fleet_hash);
        let clock = self.plot_clock();
        let span = self.span.secs();
        let watt_axis = self.held_axis(&self.watt_axis, self.axis_key(), &points, true);
        let hash_axis = self.held_axis(&self.hash_axis, self.span.key(), &hash_points, false);
        let hash_chip = hash_toggle(self.hash_on, &self.hash_chip_text(fleet_hash));
        let watts = self.header_watts();
        let watts_w = header_watts_width(&watts);
        let cost = self.header_cost();
        let today = self.header_today();
        if !self.chrome_on {
            return col(
                props!(width: width, height: height, gap: 0.0, background: Color::from_hex(0x0C_14_10)),
                [
                    chart(&points, &hash_points, clock, span, width, height, true, &watts, watt_axis, hash_axis),
                    touchable(
                        "power-chart-reveal",
                        props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                        Vec::<Draw>::new(),
                    ),
                    cost_float(&today, &cost, self.cost_on, watts_w),
                    hash_chip,
                ],
            );
        }
        let bar_h = 80.0;
        let banner_h = if self.fault.is_empty() { 0.0 } else { 64.0 };
        let chart_h = (height - bar_h - banner_h).max(120.0);
        let back_w = 120.0;
        let edge = 8.0;
        let (today_w, day_w) = measured_cost(&today, &cost, 32);
        let span_w = (width - back_w - today_w - COST_GAP - day_w - watts_w - edge).max(80.0);
        // The watts readout is a fixed canvas, right-aligned. A paragraph
        // ignores its width, so the old text sat against the chips and left
        // the spare room empty on the right.
        let mut bar = vec![
            glance_back("power-chart-close"),
            span_bar(self.span, span_w, bar_h),
            cost_readout(&today, &cost, self.cost_on, today_w, day_w, bar_h, 32),
        ];
        if watts_w > 0.0 {
            bar.push(header_readout(&watts, watts_w, bar_h));
        }
        bar.push(canvas(props!(width: edge, height: bar_h), Vec::<Draw>::new()));
        let mut rows = vec![row(
            props!(
                width: width,
                height: bar_h,
                cross_align: CrossAlign::Center,
                gap: 0.0,
                background: Color::from_rgb(41, 41, 41),
            ),
            bar,
        )];
        if !self.fault.is_empty() {
            rows.push(fault_banner(&self.fault, width));
        }
        rows.push(col(
            props!(width: width, height: chart_h, gap: 0.0),
            [
                chart(&points, &hash_points, clock, span, width, chart_h, true, "", watt_axis, hash_axis),
                touchable(
                    "power-chart-reveal",
                    props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                    Vec::<Draw>::new(),
                ),
                hash_chip,
            ],
        ));
        col(
            props!(width: width, height: height, gap: 0.0, background: Color::from_hex(0x0C_14_10)),
            rows,
        )
    }

    pub fn header_watts(&self) -> String {
        if !self.ready() {
            return String::new();
        }
        match self.total_w {
            Some(watts) => format_watts(watts),
            None => "—".to_owned(),
        }
    }

    /// Same watts the green total is showing, priced at the saved $/kWh.
    fn header_cost(&self) -> String {
        daily_cost_text(self.total_w, self.rate)
    }

    /// Energy integrated since local midnight, priced at the saved $/kWh.
    fn header_today(&self) -> String {
        today_cost_text(self.today_kwh, self.rate)
    }

    fn hash_chip_text(&self, fleet: &[(i64, f32)]) -> String {
        if !self.hash_on {
            return "Hash".to_owned();
        }
        let value = self
            .hash_sample
            .or_else(|| self.hash_ring.fine.last().map(|(_, watts)| *watts))
            .or_else(|| fleet.last().map(|(_, th)| *th));
        match value {
            Some(th) => format_hash_chip(th),
            None => "TH/s".to_owned(),
        }
    }

    /// Orange line: one point per finished refresh round, each point the sum
    /// of the miners' latest polls. Older fleet-chart samples are a different
    /// clock and are not mixed in.
    fn overlay_hash(&self, _fleet: &[(i64, f32)]) -> Vec<(i64, f32)> {
        if !self.hash_on {
            return Vec::new();
        }
        self.hash_window()
    }

    pub fn take_persist(&mut self) -> bool {
        let save = self.persist;
        self.persist = false;
        save
    }

    pub fn owns(&self, socket_id: u32) -> bool {
        self.socket.is_some_and(|socket| socket.0.to_wire() == socket_id)
    }

    pub fn owns_fetch(&self, fetch_id: u32) -> bool {
        self.fetch == Some(fetch_id) || self.quota_fetch == Some(fetch_id)
    }

    pub fn stop(&mut self) {
        if let Some(socket) = self.socket.take() {
            socket.close();
        }
        self.buf.clear();
        self.connected = false;
        self.subscribed = false;
        self.fetch = None;
        self.quota_fetch = None;
        self.ping_ms = 0;
    }

    pub fn idle(&mut self) {
        if self.socket.is_some() || self.fetch.is_some() {
            self.stop();
            self.retry_ms = 0;
        }
    }

    pub fn pump(&mut self, delta_ms: u32) {
        if !self.ready() {
            self.idle();
            self.status = "Send it from HashWatcher".to_owned();
            return;
        }
        self.load_history();
        if self.hold_ms > 0 {
            self.hold_ms = self.hold_ms.saturating_sub(delta_ms.min(2_000));
        }
        if self.chart_full && self.chrome_on {
            self.chrome_ms = self.chrome_ms.saturating_sub(delta_ms.min(2_000));
            if self.chrome_ms == 0 {
                self.chrome_on = false;
            }
        }
        if self.hold_ms == 0 {
            self.retry_ms = self.retry_ms.saturating_add(delta_ms.min(2_000));
        }
        self.poll_quota(delta_ms);
        if self.mqtt_user.is_empty() {
            if self.fetch.is_none() && self.hold_ms == 0 && self.retry_ms >= 1_000 {
                self.start_cert_fetch();
            }
        } else {
            if self.socket.is_none() && self.hold_ms == 0 && self.retry_ms >= 3_000 {
                self.open_mqtt();
            }
            if self.connected {
                self.ping_ms = self.ping_ms.saturating_add(delta_ms.min(2_000));
                if self.ping_ms >= 15_000 {
                    self.ping_ms = 0;
                    if let Some(socket) = self.socket {
                        socket.write(&[0xC0, 0x00]);
                    }
                }
            }
        }
        self.note_power();
    }

    pub fn on_fetch(&mut self, response: &FetchResponse) {
        let id = response.request_id.to_wire();
        if self.quota_fetch == Some(id) {
            self.quota_fetch = None;
            if response.ok() {
                if let Some(text) = response.text() {
                    if !self.ingest_json(text) {
                        if let Some(message) = api_fault(text) {
                            self.note_fault(message);
                        }
                    }
                }
            } else if let Some(detail) = describe_fetch(response) {
                self.note_fault(format!("EcoFlow: {detail}"));
            }
            return;
        }
        self.fetch = None;
        if !response.ok() {
            let detail = describe_fetch(response).unwrap_or_else(|| "did not answer".to_owned());
            self.fail_region(&format!("EcoFlow: {detail}"));
            return;
        }
        let doc = response.json();
        let code = doc.i64("/code").or_else(|| doc.str("/code").and_then(|text| text.parse().ok()));
        if code == Some(8513) {
            self.fail_region("Trying the other EcoFlow region");
            return;
        }
        if let Some(code) = code {
            if code != 0 && code != 200 {
                let message = doc.str("/message").or_else(|| doc.str("/msg")).unwrap_or_default();
                let message = if message.is_empty() {
                    format!("EcoFlow error {code}")
                } else {
                    format!("EcoFlow: {message}")
                };
                self.status = message.clone();
                self.note_fault(message);
                self.retry_ms = 0;
                return;
            }
        }
        let account = doc
            .str("/data/certificateAccount")
            .or_else(|| doc.str("/data/account"))
            .unwrap_or_default();
        let password = doc
            .str("/data/certificatePassword")
            .or_else(|| doc.str("/data/password"))
            .unwrap_or_default();
        let host = doc
            .str("/data/url")
            .or_else(|| doc.str("/data/host"))
            .unwrap_or_else(|| "mqtt.ecoflow.com".to_owned());
        if account.is_empty() || password.is_empty() {
            self.fail_region("EcoFlow login was empty");
            return;
        }
        self.clear_fault();
        self.mqtt_user = account;
        self.mqtt_pass = password;
        self.mqtt_host = host;
        self.topic = format!("/open/{}/{}/quota", self.mqtt_user, self.serial);
        self.status = "Connecting".to_owned();
        self.retry_ms = 3_000;
    }

    pub fn on_socket(&mut self, socket: Socket, event: &SocketEvent<'_>) {
        match event {
            SocketEvent::Connected => {
                self.connected = true;
                self.subscribed = false;
                self.ping_ms = 0;
                self.status = "Connecting".to_owned();
                socket.write(&mqtt_connect(&self.mqtt_user, &self.mqtt_pass));
            }
            SocketEvent::Data(data) => {
                self.buf.extend_from_slice(data);
                self.drain_mqtt(socket);
            }
            SocketEvent::Closed(code) => {
                self.socket = None;
                self.connected = false;
                self.subscribed = false;
                self.buf.clear();
                self.retry_ms = 0;
                // A refused login already has the real reason. Closing the
                // socket must not cover it with "Reconnecting to EcoFlow".
                if self.hold_ms > 0 || self.status == "Send it from HashWatcher" {
                    return;
                }
                self.status = "Reconnecting".to_owned();
                let detail = if *code == 0 {
                    "Reconnecting to EcoFlow".to_owned()
                } else {
                    format!("EcoFlow link dropped ({code})")
                };
                self.note_link_fault(detail);
            }
        }
    }

    pub fn set_hashrate(&mut self, th: Option<f32>, every_secs: u32) {
        self.hash_sample = th.filter(|value| value.is_finite() && *value >= 0.0);
        self.hash_every = every_secs.clamp(1, 60) as u16;
    }

    /// One orange point for a finished refresh round. The watts poll does not
    /// decide when this total moves.
    pub fn commit_hashrate(&mut self) {
        let Some(th) = self.hash_sample else {
            return;
        };
        let now = SystemTime::now().unix_secs;
        // A point already on the line keeps its value. A second sample in the
        // same second waits for the next timestamp instead of moving the tip.
        if now <= self.last_hash_sec {
            return;
        }
        let interval = i64::from(self.hash_every.max(1));
        self.hash_ring.push(now, th, interval);
        self.last_hash_sec = now;
    }

    pub fn select(&mut self, id: &str) -> bool {
        if id == "power-hash" {
            self.hash_on = !self.hash_on;
            self.persist = true;
            return true;
        }
        if id == "power-cost-day" {
            self.cost_on = !self.cost_on;
            self.persist = true;
            return true;
        }
        if id == "power-chart-close" {
            self.chart_full = false;
            self.chrome_on = false;
            return true;
        }
        if id == "power-chart-reveal" || (self.chart_full && id.starts_with("power-")) {
            self.show_chrome();
            if id == "power-chart-reveal" {
                return true;
            }
        }
        if let Some(secs) = id.strip_prefix("power-poll-") {
            if let Ok(secs) = secs.parse::<u16>() {
                let secs = normalize_poll(secs);
                if secs != self.poll_secs {
                    self.poll_secs = secs;
                    self.persist = true;
                }
                return true;
            }
        }
        if id == "power-chart-open" {
            self.chart_full = true;
            self.show_chrome();
            return true;
        }
        if id == "power-gear" {
            self.rate_text = format_rate(self.rate);
            self.cost_edit = true;
            return true;
        }
        if id == "power-gear-close" {
            self.rate_text = format_rate(self.rate);
            self.cost_edit = false;
            return true;
        }
        if id == "power-ok" {
            self.commit_rate();
            self.cost_edit = false;
            return true;
        }
        if id == "power-del" {
            self.rate_text.pop();
            return true;
        }
        if id == "power-dot" {
            self.push_rate('.');
            return true;
        }
        if let Some(digit) = id.strip_prefix("power-d") {
            if digit.len() == 1 && digit.chars().all(|ch| ch.is_ascii_digit()) {
                self.push_rate(digit.chars().next().unwrap_or('0'));
                return true;
            }
        }
        if let Some(span) = Span::from_id(id) {
            self.span = span;
            return true;
        }
        let next = match id {
            "power-total" => Series::Total,
            "power-l1" => Series::L1,
            "power-l2" => Series::L2,
            "power-l3" => Series::L3,
            _ => return false,
        };
        self.series = next;
        true
    }

    pub fn view(&self, width: f32, height: f32) -> Node {
        if !self.ready() {
            return col(
                props!(width: width, height: height, justify_content: Justify::Center, gap: 12.0),
                [
                    text(
                        "Power Monitor",
                        style!(size: 28, weight: FontWeight::BOLD, color: Color::from_hex(0x00_CC_66)),
                    ),
                    text(
                        "Send the EcoFlow power monitor from HashWatcher, then tap Update Deck inventory.",
                        style!(size: 18, color: Color::from_hex(0xC8_D4_CC), line_height: 1.3),
                    ),
                ],
            );
        }
        if self.cost_edit {
            return self.cost_page(width, height);
        }
        let title = if self.name.is_empty() { "Power Monitor".to_owned() } else { self.name.clone() };
        let total = match self.total_w {
            Some(watts) => format_watts(watts),
            None => "—".to_owned(),
        };
        let cost = if self.rate > 0.0 {
            format!("Today {}", money(self.today_kwh * self.rate))
        } else {
            "Set $/kWh".to_owned()
        };
        let button_h = 72.0;
        let fault_h = if self.fault.is_empty() { 0.0 } else { 82.0 };
        let cards_h = (height - button_h - fault_h - 28.0).max(200.0);
        let gap = 8.0;
        let total_col = (width * 0.30).clamp(200.0, 340.0);
        let card_w = ((width - total_col - gap * 3.0) / 3.0).max(72.0);
        scroll(
            "power-scroll",
            props!(width: width, height: height),
            [col(
                props!(width: width, gap: 10.0),
                [
                    row(
                        props!(width: width, height: button_h, cross_align: CrossAlign::Center, gap: 12.0),
                        [
                            chart_button(),
                            text(
                                title,
                                style!(size: 28, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF)),
                            ),
                            text(
                                format!("{}", self.status_line()),
                                style!(
                                    size: if self.fault.is_empty() { 24 } else { 28 },
                                    weight: FontWeight::BOLD,
                                    color: if self.fault.is_empty() { Color::from_hex(0x9A_B0_A4) } else { Color::from_hex(0xFF_C4_00) },
                                ),
                            ),
                            spacer(1.0),
                            gear_button(),
                        ],
                    ),
                    fault_banner(&self.fault, width),
                    row(
                        props!(width: width, height: cards_h, gap: gap, cross_align: CrossAlign::Center),
                        [
                            total_card(
                                &total,
                                &format_amps(self.total_amps()),
                                &cost,
                                self.series == Series::Total,
                                total_col,
                                cards_h,
                            ),
                            phase_card(
                                "power-l1",
                                "L1",
                                self.l1_w,
                                self.l1_a,
                                self.l1_v,
                                self.series == Series::L1,
                                card_w,
                                cards_h,
                            ),
                            phase_card(
                                "power-l2",
                                "L2",
                                self.l2_w,
                                self.l2_a,
                                self.l2_v,
                                self.series == Series::L2,
                                card_w,
                                cards_h,
                            ),
                            phase_card(
                                "power-l3",
                                "L3",
                                self.l3_w,
                                self.l3_a,
                                self.l3_v,
                                self.series == Series::L3,
                                card_w,
                                cards_h,
                            ),
                        ],
                    ),
                ],
            )],
        )
    }

    pub fn ready(&self) -> bool {
        !self.access_key.is_empty() && !self.secret_key.is_empty() && !self.serial.is_empty()
    }

    fn series_name(&self) -> &'static str {
        match self.series {
            Series::Total => "Total",
            Series::L1 => "L1",
            Series::L2 => "L2",
            Series::L3 => "L3",
        }
    }

    fn total_amps(&self) -> Option<f64> {
        let phases = [self.l1_a, self.l2_a, self.l3_a];
        if phases.iter().all(Option::is_none) {
            None
        } else {
            Some(phases.iter().flatten().sum())
        }
    }

    pub fn fault_line(&self) -> String {
        self.fault.clone()
    }

    fn status_line(&self) -> String {
        if !self.fault.is_empty() {
            return self.fault.clone();
        }
        if !self.grid.is_empty() && self.total_w.is_some() {
            return self.grid.clone();
        }
        self.status.clone()
    }

    fn show_chrome(&mut self) {
        self.chrome_on = true;
        self.chrome_ms = 5_000;
    }

    fn feed_is_fresh(&self) -> bool {
        if self.last_ok_sec == 0 {
            return false;
        }
        let window = i64::from(self.poll_secs.max(1)).saturating_mul(3).max(20);
        SystemTime::now().unix_secs.saturating_sub(self.last_ok_sec) < window
    }

    fn note_fault(&mut self, message: impl Into<String>) {
        if self.feed_is_fresh() {
            return;
        }
        self.fault = clip_fault(&message.into());
    }

    fn note_link_fault(&mut self, message: impl Into<String>) {
        if self.feed_is_fresh() {
            return;
        }
        self.note_fault(message);
    }

    fn clear_fault(&mut self) {
        self.fault.clear();
        self.last_ok_sec = SystemTime::now().unix_secs;
    }

    fn note_power(&mut self) {
        let now = SystemTime::now().unix_secs;
        self.roll_day(now);
        if self.latest.is_none() {
            return;
        }
        let interval = i64::from(self.poll_secs.max(1));
        let slot = sample_slot(now, interval);
        // A chart stamp ahead of the clock freezes both the plot and the day
        // total. Drop it and keep integrating from the live sample.
        if self.last_sample_sec > slot.saturating_add(3) {
            self.last_sample_sec = 0;
        }
        if self.last_sample_sec != 0 && slot <= self.last_sample_sec {
            return;
        }
        let cap = interval.max(15);
        let gap = if self.last_sample_sec == 0 {
            1
        } else {
            (slot - self.last_sample_sec).clamp(1, cap)
        };
        self.publish_latest();
        self.last_sample_sec = slot;
        let values = [self.total_w, self.l1_w, self.l2_w, self.l3_w];
        for (index, watts) in values.into_iter().enumerate() {
            if let Some(watts) = watts {
                self.rings[index].push(slot, watts as f32, interval);
            }
        }
        if let Some(watts) = self.total_w {
            // Consumption only. The quota is instantaneous watts, so the day
            // total is this integral. Export does not erase what was used.
            if watts > 0.0 {
                self.today_kwh += watts * (gap as f64) / 3_600_000.0;
            }
            if now.rem_euclid(60) == 0 {
                self.adopt_recorded_day(now);
            }
            if now - self.last_persist_sec >= 300 {
                self.last_persist_sec = now;
                self.persist = true;
            }
        }
        self.save_history(now);
    }

    fn roll_day(&mut self, now: i64) {
        let day = local_day(now);
        if day == 0 {
            return;
        }
        if self.day == 0 {
            self.day = day;
            return;
        }
        if day == self.day {
            return;
        }
        self.saved_cost = self.today_kwh * self.rate;
        self.saved_day = self.day;
        self.today_kwh = 0.0;
        self.day = day;
        self.persist = true;
    }

    fn plot_clock(&self) -> i64 {
        let lead = self.last_sample_sec.max(self.last_hash_sec);
        if lead > 0 {
            return lead;
        }
        sample_slot(SystemTime::now().unix_secs, i64::from(self.poll_secs.max(1)))
    }

    fn window_points(&self) -> Vec<(i64, f32)> {
        self.ring_window(&self.rings[self.series.slot()])
    }

    fn hash_window(&self) -> Vec<(i64, f32)> {
        self.ring_window(&self.hash_ring)
    }

    fn ring_window(&self, ring: &Ring) -> Vec<(i64, f32)> {
        let now = self.plot_clock();
        let start = now - self.span.secs();
        let interval = i64::from(self.poll_secs.max(1));
        let fine: Vec<(i64, f32)> = ring
            .fine
            .iter()
            .copied()
            .filter(|(stamp, _)| *stamp >= start)
            .collect();
        let fine_start = fine.first().map(|(stamp, _)| *stamp).unwrap_or(now.saturating_add(1));
        let fine_covers = fine_start <= start.saturating_add(interval);
        let mut points = Vec::new();
        if !fine_covers {
            points.extend(
                ring.coarse
                    .iter()
                    .copied()
                    .filter(|(stamp, _)| *stamp >= start && *stamp < fine_start),
            );
        }
        points.extend(fine);
        decimate_stable(&points)
    }

    fn axis_key(&self) -> u8 {
        self.span.key().saturating_mul(4).saturating_add(self.series.slot() as u8)
    }

    /// Keep the scale already on screen while new samples fit. Growing it is
    /// the only time older points move, and that happens when a sample falls
    /// outside the grid, not on every update. A trace under 800 W drops a
    /// leftover kilowatt scale so the line is not pinned to the bottom.
    fn held_axis(&self, cell: &Cell<(u8, f64, f64)>, key: u8, samples: &[(i64, f32)], watts: bool) -> (f64, f64) {
        let (min, max) = sample_bounds(samples);
        let (held_key, held_low, held_high) = cell.get();
        let held_ok = held_key == key && held_low < held_high;
        if watts && max < 800.0 {
            let data_span = (max - min).max(40.0);
            let held_span = held_high - held_low;
            let coarse = !held_ok || held_span > data_span * 2.5 || held_high > max + 150.0;
            if held_ok && !coarse && min >= held_low && max <= held_high {
                return (held_low, held_high);
            }
            let range = zoomed_watt_range(min, max);
            cell.set((key, range.0, range.1));
            return range;
        }
        if held_ok && min >= held_low && max <= held_high {
            return (held_low, held_high);
        }
        let (mut low, mut high) = if watts {
            snapped_watt_range(min, max)
        } else {
            snapped_hash_range(min, max)
        };
        if held_ok {
            low = low.min(held_low);
            high = high.max(held_high);
        }
        cell.set((key, low, high));
        (low, high)
    }

    fn load_ledger(&mut self, deck: &str) {
        if self.ledger_loaded {
            return;
        }
        self.ledger_loaded = true;
        if let Some(text) = payload_value(deck, "rate") {
            self.rate_text = text;
            self.rate = self.rate_text.parse::<f64>().unwrap_or(0.0).max(0.0);
        }
        if let Some(text) = payload_value(deck, "kwh") {
            self.today_kwh = text.parse::<f64>().unwrap_or(0.0).max(0.0);
        }
        if let Some(text) = payload_value(deck, "day") {
            self.day = text.parse::<i32>().unwrap_or(0);
        }
        if let Some(text) = payload_value(deck, "saved") {
            self.saved_cost = text.parse::<f64>().unwrap_or(0.0).max(0.0);
        }
        if let Some(text) = payload_value(deck, "sday") {
            self.saved_day = text.parse::<i32>().unwrap_or(0);
        }
        if let Some(text) = payload_value(deck, "poll") {
            self.poll_secs = normalize_poll(text.parse::<u16>().unwrap_or(1));
        }
        if let Some(text) = payload_value(deck, "hov") {
            self.hash_on = text != "0";
        }
        if let Some(text) = payload_value(deck, "cst") {
            self.cost_on = text != "0";
        }
        let today = local_day(SystemTime::now().unix_secs);
        if self.day != 0 && today != 0 && self.day != today {
            if self.saved_day != self.day {
                self.saved_cost = self.today_kwh * self.rate;
                self.saved_day = self.day;
            }
            self.today_kwh = 0.0;
            self.day = today;
            self.persist = true;
        }
    }

    fn push_rate(&mut self, ch: char) {
        if self.rate_text.len() >= 8 {
            return;
        }
        if ch == '.' && self.rate_text.contains('.') {
            return;
        }
        if ch == '.' && self.rate_text.is_empty() {
            self.rate_text.push('0');
        }
        self.rate_text.push(ch);
    }

    fn commit_rate(&mut self) {
        self.rate = self.rate_text.parse::<f64>().unwrap_or(0.0).max(0.0);
        self.rate_text = format_rate(self.rate);
        self.persist = true;
    }

    fn cost_page(&self, width: f32, height: f32) -> Node {
        let shown = if self.rate_text.is_empty() {
            "0.00".to_owned()
        } else {
            self.rate_text.clone()
        };
        let saved = if self.saved_day == 0 {
            "No saved day yet".to_owned()
        } else {
            format!("Saved daily cost  {}  {}", money(self.saved_cost), format_day(self.saved_day))
        };
        let today = format!("Today {}", money(self.today_kwh * self.rate));
        scroll(
            "power-cost-scroll",
            props!(width: width, height: height),
            [col(
                props!(width: width, gap: 8.0),
                [
                    row(
                        props!(width: width, height: 40.0, cross_align: CrossAlign::Center, gap: 12.0),
                        [
                            series_chip("power-gear-close", "Back", "", false),
                            text(
                                "Power Settings",
                                style!(size: 22, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF)),
                            ),
                            spacer(1.0),
                            series_chip("power-ok", "Save", "", true),
                        ],
                    ),
                    text(
                        "EcoFlow refresh",
                        style!(size: 16, color: Color::from_hex(0x9A_B0_A4), line_height: 1.0),
                    ),
                    poll_row(self.poll_secs, width),
                    text(
                        "$ / kWh",
                        style!(size: 16, color: Color::from_hex(0x9A_B0_A4), line_height: 1.0),
                    ),
                    text(
                        shown,
                        style!(size: 32, weight: FontWeight::BOLD, color: Color::from_hex(0x00_CC_66), line_height: 1.0),
                    ),
                    key_row(&[("power-d1", "1"), ("power-d2", "2"), ("power-d3", "3")], width),
                    key_row(&[("power-d4", "4"), ("power-d5", "5"), ("power-d6", "6")], width),
                    key_row(&[("power-d7", "7"), ("power-d8", "8"), ("power-d9", "9")], width),
                    key_row(&[("power-dot", "."), ("power-d0", "0"), ("power-del", "Del")], width),
                    text(
                        today,
                        style!(size: 20, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF), line_height: 1.0),
                    ),
                    text(
                        saved,
                        style!(size: 18, color: Color::from_hex(0x9A_B0_A4), line_height: 1.2),
                    ),
                ],
            )],
        )
    }

    fn start_cert_fetch(&mut self) {
        let region = self.region_order()[self.region_try as usize % 2];
        let host = if region == "eu" { "https://api-e.ecoflow.com" } else { "https://api.ecoflow.com" };
        let url = format!("{host}/iot-open/sign/certification");
        let timestamp = format!("{}", SystemTime::now().unix_secs.saturating_mul(1000));
        let nonce = format!("{}", 100_000 + (SystemTime::now().unix_secs % 900_000));
        let sign_string = format!(
            "accessKey={}&nonce={}&timestamp={}",
            self.access_key, nonce, timestamp
        );
        let sign = hmac_sha256_hex(self.secret_key.as_bytes(), sign_string.as_bytes());
        let headers = format!(
            "accessKey: {}\ntimestamp: {}\nnonce: {}\nsign: {}\nAccept: application/json",
            self.access_key, timestamp, nonce, sign
        );
        self.status = "Connecting".to_owned();
        self.retry_ms = 0;
        if let Some(id) = FetchRequest::get(&url)
            .headers(&headers)
            .timeout(Duration::from_secs(12))
            .send(super::on_fetch)
        {
            self.fetch = Some(id.to_wire());
        } else {
            self.note_fault("Could not start the EcoFlow login");
        }
    }

    fn region_order(&self) -> [&'static str; 2] {
        let region = self.region.trim().to_ascii_lowercase();
        if region == "eu" || region == "europe" {
            ["eu", "us"]
        } else {
            ["us", "eu"]
        }
    }

    fn fail_region(&mut self, message: &str) {
        self.region_try = self.region_try.saturating_add(1);
        self.retry_ms = 0;
        self.status = message.to_owned();
        self.note_fault(message.to_owned());
        if self.region_try >= 2 {
            self.region_try = 0;
            self.retry_ms = 0;
        }
    }

    fn open_mqtt(&mut self) {
        self.retry_ms = 0;
        let host = if self.mqtt_host.is_empty() { "mqtt.ecoflow.com" } else { &self.mqtt_host };
        if let Some(socket) = socket::tls_connect(host, 8883, super::on_socket) {
            self.socket = Some(socket);
            self.status = "Connecting".to_owned();
        } else {
            self.note_link_fault("Could not reach EcoFlow");
        }
    }

    fn drain_mqtt(&mut self, socket: Socket) {
        while let Some((kind, body)) = take_packet(&mut self.buf) {
            match kind >> 4 {
                2 => {
                    let code = body.get(1).copied().unwrap_or(1);
                    if code == 0 {
                        self.hold_ms = 0;
                        self.status = "Waiting for the monitor".to_owned();
                        socket.write(&mqtt_subscribe(&self.topic, self.packet_id));
                        self.packet_id = self.packet_id.wrapping_add(1).max(1);
                        self.subscribed = true;
                    } else {
                        // Code 5 is "not authorized". EcoFlow allows 10 unique
                        // client ids a day; a fresh id on every reconnect hits it.
                        let message = if code == 5 {
                            "EcoFlow allows 10 links a day. Waiting to retry.".to_owned()
                        } else {
                            format!("EcoFlow refused the login ({code})")
                        };
                        self.status = message.clone();
                        self.note_fault(message);
                        self.hold_ms = 60_000;
                        socket.close();
                    }
                }
                3 => {
                    if let Some(text) = mqtt_payload(&body) {
                        self.ingest_json(&text);
                    }
                }
                _ => {}
            }
        }
    }

    fn poll_quota(&mut self, delta_ms: u32) {
        self.quota_ms = self.quota_ms.saturating_add(delta_ms.min(2_000));
        let every = u32::from(self.poll_secs.max(1)).saturating_mul(1_000);
        if self.quota_ms < every || self.quota_fetch.is_some() || self.fetch.is_some() {
            return;
        }
        self.quota_ms = 0;
        self.start_quota_fetch();
    }

    fn start_quota_fetch(&mut self) {
        if self.access_key.is_empty() || self.secret_key.is_empty() || self.serial.is_empty() {
            return;
        }
        let region = self.region_order()[0];
        let host = if region == "eu" { "https://api-e.ecoflow.com" } else { "https://api.ecoflow.com" };
        let timestamp = format!("{}", SystemTime::now().unix_secs.saturating_mul(1000));
        let nonce = format!("{}", 100_000 + (SystemTime::now().unix_secs % 900_000));
        // Match the phone client: query fields first, then accessKey, nonce, timestamp.
        // Sorting `sn` into the middle makes EcoFlow reject the signature.
        let sign_string = format!(
            "sn={}&accessKey={}&nonce={}&timestamp={}",
            self.serial, self.access_key, nonce, timestamp
        );
        let sign = hmac_sha256_hex(self.secret_key.as_bytes(), sign_string.as_bytes());
        let headers = format!(
            "accessKey: {}\ntimestamp: {}\nnonce: {}\nsign: {}\nAccept: application/json",
            self.access_key, timestamp, nonce, sign
        );
        let url = format!("{host}/iot-open/sign/device/quota/all?sn={}", self.serial);
        if let Some(id) = FetchRequest::get(&url)
            .headers(&headers)
            .timeout(Duration::from_secs(12))
            .send(super::on_fetch)
        {
            self.quota_fetch = Some(id.to_wire());
        } else {
            self.note_fault("Could not start the EcoFlow request");
        }
    }

    fn load_history(&mut self) {
        if self.history_loaded {
            return;
        }
        self.history_loaded = true;
        let Some(bytes) = read_power_chart() else {
            return;
        };
        if !restore_power_chart(&mut self.rings, &mut self.hash_ring, &bytes) {
            return;
        }
        let now = SystemTime::now().unix_secs;
        let interval = i64::from(self.poll_secs.max(1));
        // A clock that has not been set yet must not throw away a saved chart.
        let sane = now > 1_704_067_200;
        let newest = self.rings[0]
            .fine
            .last()
            .map(|(stamp, _)| *stamp)
            .or_else(|| self.rings[0].coarse.last().map(|(stamp, _)| *stamp))
            .unwrap_or(0);
        let horizon = if sane { now } else { now.max(newest) };
        for ring in &mut self.rings {
            trim_ring(ring, horizon, interval, sane);
            backfill_coarse(ring);
        }
        trim_ring(&mut self.hash_ring, horizon, interval, sane);
        backfill_coarse(&mut self.hash_ring);
        if let Some((stamp, _)) = self.hash_ring.fine.last().copied() {
            self.last_hash_sec = stamp;
        }
        if newest > 0 && (!sane || newest <= now.saturating_add(180)) {
            self.last_sample_sec = newest;
        }
        if self.total_w.is_none() {
            if let Some((_, watts)) = self
                .rings[0]
                .fine
                .last()
                .copied()
                .or_else(|| self.rings[0].coarse.last().copied())
            {
                self.total_w = Some(f64::from(watts));
            }
        }
        self.last_history_save = now;
        self.adopt_recorded_day(now);
    }

    /// The saved counter can sit at whatever was last written. The chart
    /// already holds today's watts, so take that when it is ahead.
    fn adopt_recorded_day(&mut self, now: i64) {
        let summed = ring_consumption_kwh(
            &self.rings[0],
            local_midnight(now),
            i64::from(self.poll_secs.max(1)),
        );
        if summed > self.today_kwh + 0.000_01 {
            self.today_kwh = summed;
            self.persist = true;
        }
    }

    fn save_history(&mut self, now: i64) {
        if self.last_history_save != 0 && now.saturating_sub(self.last_history_save) < HISTORY_SAVE_SECS {
            return;
        }
        let bytes = pack_power_chart(&self.rings, &self.hash_ring);
        if bytes.is_empty() {
            return;
        }
        // One overwrite of the same cache key. The host stores it under the
        // widget's flash bucket, atomically, and skips the rewrite when the
        // bytes have not changed. Ten minutes keeps a power cut from wiping
        // the chart without a write on every sample.
        bmc_wasm_sdk::cache::put("power-chart", b"", &bytes);
        self.last_history_save = now;
    }

    fn ingest_json(&mut self, text: &str) -> bool {
        let l1 = json_number(text, "gridConnectionPowerL1");
        let l2 = json_number(text, "gridConnectionPowerL2");
        let l3 = json_number(text, "gridConnectionPowerL3");
        let total = json_number(text, "powGetSysGrid")
            .or_else(|| json_number(text, "gridConnectionPower"))
            .or_else(|| match (l1, l2, l3) {
                (None, None, None) => None,
                _ => Some(l1.unwrap_or(0.0) + l2.unwrap_or(0.0) + l3.unwrap_or(0.0)),
            });
        let l1_v = json_number_any(text, &["gridConnectionVolL1", "voltageL1", "gridVolL1"]);
        let l2_v = json_number_any(text, &["gridConnectionVolL2", "voltageL2", "gridVolL2"]);
        let l3_v = json_number_any(text, &["gridConnectionVolL3", "voltageL3", "gridVolL3"]);
        let l1_a = json_number_any(text, &["gridConnectionAmpL1", "currentL1", "gridAmpL1"]);
        let l2_a = json_number_any(text, &["gridConnectionAmpL2", "currentL2", "gridAmpL2"]);
        let l3_a = json_number_any(text, &["gridConnectionAmpL3", "currentL3", "gridAmpL3"]);
        let grid = json_string(text, "gridConnectionSta");
        if total.is_none() && l1.is_none() && l2.is_none() && l3.is_none()
            && l1_v.is_none() && l2_v.is_none() && l3_v.is_none()
            && l1_a.is_none() && l2_a.is_none() && l3_a.is_none()
            && grid.is_none()
        {
            return false;
        }
        let mut reading = self.latest.clone().unwrap_or_else(Reading::blank);
        keep(&mut reading.total_w, total);
        keep(&mut reading.l1_w, l1);
        keep(&mut reading.l2_w, l2);
        keep(&mut reading.l3_w, l3);
        keep(&mut reading.l1_v, l1_v);
        keep(&mut reading.l2_v, l2_v);
        keep(&mut reading.l3_v, l3_v);
        keep(&mut reading.l1_a, l1_a);
        keep(&mut reading.l2_a, l2_a);
        keep(&mut reading.l3_a, l3_a);
        if let Some(value) = grid {
            reading.grid = match value.as_str() {
                "PANEL_GRID_IN" => "Importing".to_owned(),
                value if value.contains("GRID_OUT") => "Exporting".to_owned(),
                value if !value.is_empty() => value.to_owned(),
                _ => "Live".to_owned(),
            };
        }
        self.latest = Some(reading);
        self.clear_fault();
        true
    }

    fn publish_latest(&mut self) {
        let Some(reading) = self.latest.clone() else {
            return;
        };
        self.total_w = reading.total_w;
        self.l1_w = reading.l1_w;
        self.l2_w = reading.l2_w;
        self.l3_w = reading.l3_w;
        self.l1_v = reading.l1_v.map(mains_volts);
        self.l2_v = reading.l2_v.map(mains_volts);
        self.l3_v = reading.l3_v.map(mains_volts);
        self.l1_a = leg_amps(reading.l1_a, reading.l1_w, self.l1_v);
        self.l2_a = leg_amps(reading.l2_a, reading.l2_w, self.l2_v);
        self.l3_a = leg_amps(reading.l3_a, reading.l3_w, self.l3_v);
        if !reading.grid.is_empty() {
            self.grid = reading.grid;
            self.status = self.grid.clone();
        }
    }
}

fn fault_banner(message: &str, width: f32) -> Node {
    if message.is_empty() {
        return col(props!(width: width, height: 0.0), Vec::<Node>::new());
    }
    col(
        props!(
            width: width,
            height: 72.0,
            justify_content: Justify::Center,
            padding: 12.0,
            background: Color::from_rgba(90, 48, 0, 220),
            border_radius: 16.0,
        ),
        [text(
            message,
            style!(size: 32, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_C4_00), line_height: 1.05),
        )],
    )
}

fn clip_fault(message: &str) -> String {
    let message = message.trim();
    let mut out = String::new();
    for (index, ch) in message.chars().enumerate() {
        if index == 72 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn api_fault(body: &str) -> Option<String> {
    let message = json_string(body, "message").or_else(|| json_string(body, "msg"))?;
    let trimmed = message.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("success") || trimmed == "0" {
        None
    } else {
        Some(clip_fault(&format!("EcoFlow: {trimmed}")))
    }
}

fn describe_fetch(response: &FetchResponse) -> Option<String> {
    let body = response.text().unwrap_or("");
    let api = json_string(body, "message")
        .or_else(|| json_string(body, "msg"))
        .filter(|text| {
            let trimmed = text.trim();
            !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("success")
        });
    let detail = match response.status {
        0 => "no network".to_owned(),
        100..=999 => api.unwrap_or_else(|| format!("HTTP {}", response.status)),
        1000 => "response too large".to_owned(),
        1001 => "request blocked".to_owned(),
        1002 => return None,
        _ => api.unwrap_or_else(|| "failed".to_owned()),
    };
    Some(clip_fault(&detail))
}

fn monitor_json(text: &str) -> String {
    for line in text.lines() {
        let Some(body) = line.trim().strip_prefix(MARKER) else {
            continue;
        };
        let body = body.trim().trim_start_matches('\t').trim();
        if body.starts_with('{') {
            return body.to_owned();
        }
    }
    String::new()
}

fn payload_value(raw: &str, key: &str) -> Option<String> {
    raw.split(';').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name == key).then(|| value.trim().to_owned())
    })
}

fn total_card(watts: &str, amps: &str, cost: &str, on: bool, width: f32, height: f32) -> Node {
    let border = if on { Color::from_hex(0x00_CC_66) } else { Color::from_rgba(255, 255, 255, 28) };
    let watt_size = ((height * 0.30) as u32).clamp(48, 84);
    let amp_size = ((height * 0.24) as u32).clamp(36, 68);
    let cost_size = ((height * 0.11) as u32).clamp(18, 28);
    col(
        props!(
            width: width,
            height: height,
            background: Color::from_rgba(8, 20, 14, 230),
            border_radius: 12.0,
            border_width: 2.0,
            border_color: border,
            padding: 10.0,
            justify_content: Justify::Center,
            gap: 6.0,
        ),
        [
            text(
                watts,
                style!(size: watt_size, weight: FontWeight::BOLD, color: Color::from_hex(0x00_CC_66), line_height: 1.0),
            ),
            text(
                amps,
                style!(size: amp_size, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF), line_height: 1.0),
            ),
            text(
                cost,
                style!(size: cost_size, color: Color::from_hex(0x9A_B0_A4), line_height: 1.0),
            ),
            touchable(
                "power-total",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn phase_card(
    id: &str,
    label: &str,
    watts: Option<f64>,
    amps: Option<f64>,
    volts: Option<f64>,
    on: bool,
    width: f32,
    height: f32,
) -> Node {
    let border = if on { Color::from_hex(0x00_CC_66) } else { Color::from_rgba(255, 255, 255, 28) };
    let label_size = ((height * 0.10) as u32).clamp(18, 28);
    let watt_size = ((height * 0.22) as u32).clamp(28, 52);
    let amp_size = ((height * 0.28) as u32).clamp(40, 72);
    let volt_size = ((height * 0.13) as u32).clamp(18, 32);
    col(
        props!(
            width: width,
            height: height,
            background: Color::from_rgba(20, 28, 24, 220),
            border_radius: 12.0,
            border_width: 2.0,
            border_color: border,
            padding: 10.0,
            justify_content: Justify::Center,
            gap: 6.0,
        ),
        [
            text(
                label,
                style!(size: label_size, weight: FontWeight::BOLD, color: Color::from_hex(0x9A_B0_A4), line_height: 1.0),
            ),
            text(
                watts.map(format_watts).unwrap_or_else(|| "—".to_owned()),
                style!(size: watt_size, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF), line_height: 1.0),
            ),
            text(
                format_amps(amps),
                style!(size: amp_size, weight: FontWeight::BOLD, color: Color::from_hex(0x00_CC_66), line_height: 1.0),
            ),
            text(
                volts.map(format_volts).unwrap_or_else(|| "—".to_owned()),
                style!(size: volt_size, weight: FontWeight::SEMIBOLD, color: Color::from_hex(0xC8_D4_CC), line_height: 1.0),
            ),
            touchable(id, props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0), Vec::<Draw>::new()),
        ],
    )
}

fn leg_detail(amps: Option<f64>, volts: Option<f64>) -> String {
    match (amps, volts) {
        (Some(amps), Some(volts)) => format!("{}   {}", format_amps(Some(amps)), format_volts(volts)),
        (Some(amps), None) => format_amps(Some(amps)),
        (None, Some(volts)) => format_volts(volts),
        (None, None) => "—".to_owned(),
    }
}

fn format_volts(volts: f64) -> String {
    format!("{volts:.0} V")
}

fn mains_volts(volts: f64) -> f64 {
    if volts.abs() > 1000.0 { volts / 1000.0 } else { volts }
}

fn leg_amps(reported: Option<f64>, watts: Option<f64>, volts: Option<f64>) -> Option<f64> {
    if reported.is_some() {
        return reported;
    }
    match (watts, volts) {
        (Some(watts), Some(volts)) if volts.abs() > 20.0 => Some(watts / volts),
        _ => None,
    }
}

fn keep(slot: &mut Option<f64>, value: Option<f64>) {
    if value.is_some() {
        *slot = value;
    }
}

fn json_number_any(raw: &str, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| json_number(raw, key))
}

fn series_chip(id: &str, label: &str, subtitle: &str, on: bool) -> Node {
    let fill = if on { Color::from_hex(0x00_CC_66) } else { Color::from_rgba(255, 255, 255, 28) };
    let ink = if on { Color::from_hex(0x04_12_0C) } else { Color::from_hex(0xFF_FF_FF) };
    let sub = if on { Color::from_hex(0x04_12_0C) } else { Color::from_hex(0xC8_D4_CC) };
    let height = if subtitle.is_empty() { 36.0 } else { 56.0 };
    let mut lines = vec![text(
        label,
        style!(size: 16, weight: FontWeight::BOLD, color: ink, line_height: 1.0),
    )];
    if !subtitle.is_empty() {
        lines.push(text(
            subtitle,
            style!(size: 14, weight: FontWeight::SEMIBOLD, color: sub, line_height: 1.0),
        ));
    }
    lines.push(touchable(
        id,
        props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
        Vec::<Draw>::new(),
    ));
    col(
        props!(
            width: 110.0,
            height: height,
            background: fill,
            border_radius: 18.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
            gap: 2.0,
        ),
        lines,
    )
}

fn glance_back(id: &str) -> Node {
    col(
        props!(
            width: 120.0,
            height: 72.0,
            background: TRANSPARENT,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                "Back",
                style!(size: 24, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF), line_height: 1.0, align: TextAlign::Center),
            ),
            touchable(id, props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0), Vec::<Draw>::new()),
        ],
    )
}

fn close_mark(id: &str) -> Node {
    col(
        props!(
            width: 40.0,
            height: 40.0,
            background: Color::from_rgba(255, 255, 255, 28),
            border_radius: 20.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                "×",
                style!(size: 26, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF), line_height: 1.0, align: TextAlign::Center),
            ),
            touchable(id, props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0), Vec::<Draw>::new()),
        ],
    )
}

fn sample_slot(now: i64, interval: i64) -> i64 {
    let interval = interval.max(1);
    if interval <= 1 {
        return now;
    }
    now - now.rem_euclid(interval)
}

fn fine_keep_secs(interval: i64) -> i64 {
    let interval = interval.max(1);
    (MAX_FINE_POINTS as i64)
        .saturating_mul(interval)
        .clamp(30 * 60, COARSE_KEEP_SECS)
}

fn normalize_poll(secs: u16) -> u16 {
    match secs {
        1 | 5 | 10 | 15 | 30 | 60 => secs,
        _ => 1,
    }
}

fn poll_row(selected: u16, width: f32) -> Node {
    let choices = [
        (1_u16, "1s", "power-poll-1"),
        (5, "5s", "power-poll-5"),
        (10, "10s", "power-poll-10"),
        (15, "15s", "power-poll-15"),
        (30, "30s", "power-poll-30"),
        (60, "60s", "power-poll-60"),
    ];
    let gap = 8.0;
    let chip_w = ((width - gap * (choices.len() - 1) as f32) / choices.len() as f32).max(36.0);
    row(
        props!(width: width, height: 44.0, gap: gap),
        choices
            .iter()
            .map(|(secs, label, id)| chip(id, label, *secs == selected, chip_w, 44.0))
            .collect::<Vec<_>>(),
    )
}

fn format_amps(amps: Option<f64>) -> String {
    match amps {
        Some(value) => format!("{value:.1} A"),
        None => "—".to_owned(),
    }
}

fn header_watts_width(label: &str) -> f32 {
    if label.is_empty() {
        return 0.0;
    }
    // 0.62em matches the axis gutter for this face at 56px, plus a few pixels
    // so the right-aligned glyphs do not clip into the last time chip.
    label.chars().count() as f32 * 56.0 * 0.62 + 4.0
}

fn measured_cost(today: &str, day: &str, size: u32) -> (f32, f32) {
    let em = size as f32 * 0.62;
    (
        today.chars().count() as f32 * em + 2.0,
        day.chars().count() as f32 * em + 2.0,
    )
}

fn cost_float(today: &str, day: &str, shown: bool, watts_w: f32) -> Node {
    let (today_w, day_w) = measured_cost(today, day, 40);
    let width = today_w + COST_GAP + day_w;
    col(
        props!(
            width: width,
            height: 64.0,
            inset_top: 4.0,
            inset_right: watts_w + 16.0,
        ),
        [cost_readout(today, day, shown, today_w, day_w, 64.0, 40)],
    )
}

fn cost_readout(
    today: &str,
    day: &str,
    shown: bool,
    today_w: f32,
    day_w: f32,
    height: f32,
    size: u32,
) -> Node {
    let width = today_w + COST_GAP + day_w;
    let y = ((height - size as f32) * 0.5).max(0.0);
    let draws = if shown {
        vec![
            money_text(today_w, y, today, size),
            money_text(width, y, day, size),
        ]
    } else {
        Vec::new()
    };
    col(
        props!(width: width, height: height),
        [
            canvas(props!(width: width, height: height), draws),
            touchable(
                "power-cost-day",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn money_text(x: f32, y: f32, label: &str, size: u32) -> Draw {
    let hint = label.starts_with("Set") || label == "—";
    let color = if hint {
        Color::from_hex(0x9A_B0_A4)
    } else {
        Color::from_hex(0xFF_FF_FF)
    };
    Draw::text(
        x,
        y,
        label.to_owned(),
        style!(
            size: size,
            weight: FontWeight::BOLD,
            color: color,
            align: TextAlign::Right,
            line_height: 1.0,
        ),
    )
}

fn header_readout(label: &str, width: f32, height: f32) -> Node {
    let y = ((height - 56.0) * 0.5).max(0.0);
    canvas(
        props!(width: width, height: height),
        [Draw::text(
            width,
            y,
            label.to_owned(),
            style!(
                size: 56,
                weight: FontWeight::BOLD,
                color: Color::from_hex(0x00_CC_66),
                align: TextAlign::Right,
                line_height: 1.0,
            ),
        )],
    )
}

fn hash_toggle(on: bool, label: &str) -> Node {
    let width = (label.chars().count().max(4) as f32) * 22.0 * 0.62 + 28.0;
    let fill = if on { Color::from_hex(0xFF_9F_0A) } else { Color::from_rgba(255, 255, 255, 36) };
    let ink = if on { Color::from_hex(0x1A_0E_00) } else { Color::from_hex(0xFF_FF_FF) };
    col(
        props!(
            width: width,
            height: 46.0,
            inset_top: 8.0,
            inset_left: 8.0,
            background: fill,
            border_radius: 14.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
            padding: 6.0,
        ),
        [
            text(
                label,
                style!(size: 22, weight: FontWeight::BOLD, color: ink, line_height: 1.0, align: TextAlign::Center),
            ),
            touchable(
                "power-hash",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

/// Same on-screen spacing as the 5-minute chart: about four rules, however
/// wide the watt range is. A longer window must not pack the plot with lines.
fn watt_grid_step(min_w: f64, max_w: f64, inner_h: f64) -> f64 {
    let span = (max_w - min_w).abs().max(1.0);
    let gaps = (inner_h / 88.0).clamp(3.0, 4.0);
    let mut raw = (span / gaps).max(5.0);
    let mut step = nice_watt_step(raw);
    for _ in 0..4 {
        let low = (min_w / step).floor() * step;
        let high = (max_w / step).ceil() * step;
        let lines = ((high - low) / step).round() + 1.0;
        // Five rules is the 5-minute chart. More than that is the stripe field.
        if lines <= 5.0 {
            break;
        }
        let next = nice_watt_step((step * 2.0).max(raw + step));
        if next <= step + 0.01 {
            break;
        }
        raw = next;
        step = next;
    }
    step
}

/// Smallest 1, 2, 2.5, or 5 × 10ⁿ that is at least `raw`.
fn nice_watt_step(raw: f64) -> f64 {
    let raw = raw.max(5.0);
    let exp = raw.log10().floor();
    let base = 10f64.powf(exp);
    let frac = raw / base;
    let nice = if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 2.5 {
        2.5
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * base
}

fn hash_bounds(samples: &[(i64, f32)]) -> (f64, f64) {
    let mut min = samples.first().map(|(_, value)| f64::from(*value)).unwrap_or(0.0);
    let mut max = min;
    for (_, sample) in samples {
        let value = f64::from(*sample);
        min = min.min(value);
        max = max.max(value);
    }
    let pad = ((max - min).abs() * 0.12).max(0.05);
    let low = (min - pad).max(0.0);
    let mut high = max + pad;
    if high <= low {
        high = low + 0.1;
    }
    (low, high)
}

fn format_th_axis(value: f64) -> String {
    if value >= 1000.0 {
        format!("{:.2} PH", value / 1000.0)
    } else if value >= 100.0 {
        format!("{value:.0} TH")
    } else if value >= 10.0 {
        format!("{value:.1} TH")
    } else {
        format!("{value:.2} TH")
    }
}

fn format_hash_chip(value: f32) -> String {
    format_th_axis(f64::from(value))
}

fn stroke_orange(draws: &mut Vec<Draw>, segment: &[(f32, f32)], color: Color) {
    if segment.len() >= 2 {
        draws.push(path!(segment.to_vec(), stroke: 4.0, color: color));
    } else if let Some((x, y)) = segment.first().copied() {
        draws.push(Draw::circle(x, y, 4.0, color));
    }
}

const CHART_TAG: &str = "power-chart";

fn read_power_chart() -> Option<Vec<u8>> {
    if let Some(entry) = bmc_wasm_sdk::cache::read_bytes(CHART_TAG) {
        if entry.bytes.len() >= 8 {
            return Some(entry.bytes);
        }
    }
    bmc_wasm_sdk::kv::get(CHART_TAG)
}

fn pack_power_chart(rings: &[Ring; 4], hash: &Ring) -> Vec<u8> {
    let fine_n = rings[0].fine.len().min(PERSIST_FINE_MAX);
    let coarse_n = rings[0].coarse.len();
    let hash_fine_n = hash.fine.len().min(PERSIST_FINE_MAX);
    let hash_coarse_n = hash.coarse.len();
    if fine_n == 0 && coarse_n == 0 && hash_fine_n == 0 && hash_coarse_n == 0 {
        return Vec::new();
    }
    let mut bytes = Vec::with_capacity(20 + (fine_n + coarse_n) * 24 + (hash_fine_n + hash_coarse_n) * 12);
    bytes.extend_from_slice(b"PW4");
    bytes.push(1);
    bytes.extend_from_slice(&(fine_n as u32).to_le_bytes());
    bytes.extend_from_slice(&(coarse_n as u32).to_le_bytes());
    bytes.extend_from_slice(&(hash_fine_n as u32).to_le_bytes());
    bytes.extend_from_slice(&(hash_coarse_n as u32).to_le_bytes());
    let fine_start = rings[0].fine.len() - fine_n;
    write_watt_series(&mut bytes, rings, fine_start, false);
    write_watt_series(&mut bytes, rings, rings[0].coarse.len() - coarse_n, true);
    let hash_fine_start = hash.fine.len() - hash_fine_n;
    write_hash_series(&mut bytes, &hash.fine[hash_fine_start..]);
    let hash_coarse_start = hash.coarse.len() - hash_coarse_n;
    write_hash_series(&mut bytes, &hash.coarse[hash_coarse_start..]);
    bytes
}

fn write_watt_series(bytes: &mut Vec<u8>, rings: &[Ring; 4], start: usize, coarse: bool) {
    let base = if coarse { &rings[0].coarse } else { &rings[0].fine };
    for (stamp, _) in &base[start..] {
        bytes.extend_from_slice(&stamp.to_le_bytes());
        for ring in rings {
            let series = if coarse { &ring.coarse } else { &ring.fine };
            let watts = match series.binary_search_by_key(stamp, |(sample_at, _)| *sample_at) {
                Ok(found) => series[found].1,
                Err(_) => f32::NAN,
            };
            bytes.extend_from_slice(&watts.to_le_bytes());
        }
    }
}

fn write_hash_series(bytes: &mut Vec<u8>, series: &[(i64, f32)]) {
    for (stamp, value) in series {
        bytes.extend_from_slice(&stamp.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn restore_power_chart(rings: &mut [Ring; 4], hash: &mut Ring, bytes: &[u8]) -> bool {
    if bytes.len() >= 8 && &bytes[0..3] == b"PW3" {
        let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])) as usize;
        let mut offset = 8;
        read_watt_series(bytes, &mut offset, count, rings, false);
        return offset > 8 || count == 0;
    }
    if bytes.len() < 20 || &bytes[0..3] != b"PW4" {
        return false;
    }
    let fine_n = u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])) as usize;
    let coarse_n = u32::from_le_bytes(bytes[8..12].try_into().unwrap_or([0; 4])) as usize;
    let hash_fine_n = u32::from_le_bytes(bytes[12..16].try_into().unwrap_or([0; 4])) as usize;
    let hash_coarse_n = u32::from_le_bytes(bytes[16..20].try_into().unwrap_or([0; 4])) as usize;
    let mut offset = 20;
    read_watt_series(bytes, &mut offset, fine_n, rings, false);
    read_watt_series(bytes, &mut offset, coarse_n, rings, true);
    read_hash_series(bytes, &mut offset, hash_fine_n, &mut hash.fine);
    read_hash_series(bytes, &mut offset, hash_coarse_n, &mut hash.coarse);
    true
}

fn read_watt_series(bytes: &[u8], offset: &mut usize, count: usize, rings: &mut [Ring; 4], coarse: bool) {
    for _ in 0..count {
        if *offset + 24 > bytes.len() {
            return;
        }
        let stamp = i64::from_le_bytes(bytes[*offset..*offset + 8].try_into().unwrap_or([0; 8]));
        *offset += 8;
        for ring in rings.iter_mut() {
            let watts = f32::from_le_bytes(bytes[*offset..*offset + 4].try_into().unwrap_or([0; 4]));
            *offset += 4;
            if watts.is_finite() {
                let series = if coarse { &mut ring.coarse } else { &mut ring.fine };
                push_sample(series, stamp, watts);
            }
        }
    }
}

fn read_hash_series(bytes: &[u8], offset: &mut usize, count: usize, series: &mut Vec<(i64, f32)>) {
    for _ in 0..count {
        if *offset + 12 > bytes.len() {
            return;
        }
        let stamp = i64::from_le_bytes(bytes[*offset..*offset + 8].try_into().unwrap_or([0; 8]));
        *offset += 8;
        let value = f32::from_le_bytes(bytes[*offset..*offset + 4].try_into().unwrap_or([0; 4]));
        *offset += 4;
        if value.is_finite() {
            push_sample(series, stamp, value);
        }
    }
}

fn push_sample(series: &mut Vec<(i64, f32)>, stamp: i64, value: f32) {
    if series.last().is_some_and(|(prev, _)| *prev >= stamp) {
        return;
    }
    series.push((stamp, value));
}

fn trim_ring(ring: &mut Ring, horizon: i64, interval: i64, drop_future: bool) {
    let fine_cut = horizon.saturating_sub(fine_keep_secs(interval));
    let coarse_cut = horizon.saturating_sub(COARSE_KEEP_SECS);
    let future = if drop_future { horizon.saturating_add(180) } else { i64::MAX };
    ring.fine.retain(|(stamp, _)| *stamp >= fine_cut && *stamp <= future);
    ring.coarse.retain(|(stamp, _)| *stamp >= coarse_cut && *stamp <= future);
    if ring.fine.len() > MAX_FINE_POINTS {
        let drop = ring.fine.len() - MAX_FINE_POINTS;
        ring.fine.drain(0..drop);
    }
}

fn backfill_coarse(ring: &mut Ring) {
    if !ring.coarse.is_empty() || ring.fine.len() < 2 {
        return;
    }
    let mut minute = ring.fine[0].0.div_euclid(60);
    let mut sum = 0.0_f32;
    let mut count = 0_u16;
    for (stamp, watts) in ring.fine.iter().copied() {
        let next = stamp.div_euclid(60);
        if next != minute {
            if count > 0 {
                ring.coarse.push((minute * 60, sum / f32::from(count)));
            }
            minute = next;
            sum = 0.0;
            count = 0;
        }
        sum += watts;
        count = count.saturating_add(1);
    }
}

fn sample_bounds(samples: &[(i64, f32)]) -> (f64, f64) {
    let mut min = samples.first().map(|(_, value)| f64::from(*value)).unwrap_or(0.0);
    let mut max = min;
    for (_, sample) in samples {
        let value = f64::from(*sample);
        min = min.min(value);
        max = max.max(value);
    }
    (min, max)
}

/// Axis for a trace that stays under 800 W. The band follows the samples
/// instead of starting at 0 and running up to the next kilowatt.
fn zoomed_watt_range(min: f64, max: f64) -> (f64, f64) {
    let max = max.max(0.0);
    let min = min.min(max).max(0.0);
    let span = (max - min).max(40.0);
    let mid = (min + max) * 0.5;
    let mut low = (mid - span * 0.75).max(0.0);
    let mut high = mid + span * 0.75;
    if min < 30.0 {
        low = 0.0;
    }
    let step = fine_watt_step(high - low);
    low = (low / step).floor() * step;
    high = (high / step).ceil() * step;
    if high <= low + step {
        high = low + step * 4.0;
    }
    if max >= high - step * 0.15 {
        high += step;
    }
    if low > 0.0 && min <= low + step * 0.15 {
        low = (low - step).max(0.0);
    }
    (low, high)
}

fn fine_watt_step(span: f64) -> f64 {
    let raw = (span / 4.0).max(5.0);
    let step = nice_watt_step(raw);
    if span < 250.0 {
        step.min(20.0)
    } else if span < 500.0 {
        step.min(50.0)
    } else {
        step.min(100.0)
    }
}

fn snapped_watt_range(min: f64, max: f64) -> (f64, f64) {
    let step = watt_grid_step(min, max, 320.0);
    let low = (min / step).floor() * step;
    let mut high = (max / step).ceil() * step;
    if high <= low + 0.5 {
        high = low + step;
    }
    if max >= high - step * 0.05 {
        high += step;
    }
    (low, high)
}

fn snapped_hash_range(min: f64, max: f64) -> (f64, f64) {
    let (low, high) = snapped_plot_range(min, max);
    (low.max(0.0), high.max(low.max(0.0) + 0.05))
}

pub(crate) fn snapped_plot_range(min: f64, max: f64) -> (f64, f64) {
    let span = (max - min).abs().max(0.05);
    let step = nice_step(span / 4.0);
    let low = (min / step).floor() * step;
    let mut high = (max / step).ceil() * step;
    if high <= low {
        high = low + step;
    }
    if max >= high - step * 0.05 {
        high += step;
    }
    (low, high)
}

fn nice_step(raw: f64) -> f64 {
    let raw = raw.max(0.01);
    let exp = raw.log10().floor();
    let base = 10f64.powf(exp);
    let frac = raw / base;
    let nice = if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 2.5 {
        2.5
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * base
}

/// Keep the first sample in each fixed time bucket. Appending a sample does
/// not change which older samples the line uses.
pub(crate) fn decimate_stable(points: &[(i64, f32)]) -> Vec<(i64, f32)> {
    if points.len() <= DRAW_POINTS || DRAW_POINTS < 2 {
        return points.to_vec();
    }
    let span = (points[points.len() - 1].0 - points[0].0).max(1);
    let bucket = nice_time_bucket((span / DRAW_POINTS as i64).max(1));
    let mut out = Vec::with_capacity(DRAW_POINTS + 2);
    let mut open: Option<i64> = None;
    let last = points.len() - 1;
    for (index, point) in points.iter().copied().enumerate() {
        let id = point.0.div_euclid(bucket);
        if open != Some(id) {
            out.push(point);
            open = Some(id);
        } else if index == last {
            out.push(point);
        }
    }
    out
}

fn nice_time_bucket(raw: i64) -> i64 {
    const STEPS: &[i64] = &[
        1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 14400, 21600, 43200, 86400,
    ];
    STEPS.iter().copied().find(|step| *step >= raw).unwrap_or(86_400)
}

fn chart(
    samples: &[(i64, f32)],
    hash: &[(i64, f32)],
    now: i64,
    span_secs: i64,
    width: f32,
    height: f32,
    full: bool,
    readout: &str,
    watt_axis: (f64, f64),
    hash_axis: (f64, f64),
) -> Node {
    let span = span_secs.max(1);
    let start = now.saturating_sub(span);
    let label_size: u32 = if full { 36 } else { 24 };
    let bottom = if full { 52.0 } else { 36.0 };
    let plot_h = (height - bottom).max(20.0);
    // The hashrate chip sits on the top-left of a full chart.
    let plot_top = if full && !readout.is_empty() { 72.0 } else if full { 64.0 } else { 6.0 };
    let plot_bottom = if full { plot_h.max(plot_top + 1.0) } else { (plot_h - 6.0).max(plot_top + 1.0) };
    let inner_h = plot_bottom - plot_top;
    let mut min = samples.first().map(|(_, watts)| *watts).unwrap_or(0.0);
    let mut max = min;
    for (_, sample) in samples {
        min = min.min(*sample);
        max = max.max(*sample);
    }
    let grid_step = if full {
        watt_grid_step(watt_axis.0, watt_axis.1, 320.0)
    } else {
        250.0
    };
    let (low, high) = if full {
        watt_axis
    } else {
        let pad = ((max - min).abs() * 0.12).max(1.0);
        (f64::from(min) - f64::from(pad), f64::from(max) + f64::from(pad))
    };
    let scale = (high - low).max(1.0);
    // Fixed gutters. Measuring the current label and resizing the plot moves
    // every point that was already drawn.
    let gutter = if full {
        196.0
    } else {
        axis_gutter(&axis_watts(high, grid_step), label_size).max(axis_gutter(&axis_watts(low, grid_step), label_size))
    };
    let show_hash = full && hash.len() >= 2;
    let (hash_low, hash_high) = if full { hash_axis } else { hash_bounds(hash) };
    let hash_gutter = if show_hash { 148.0 } else { 0.0 };
    let left = if full { 8.0 + hash_gutter } else { gutter };
    let right = if full { gutter } else { 8.0 };
    let plot_w = (width - left - right).max(20.0);
    let grid = Color::from_rgba(255, 255, 255, 150);
    let grid_strong = Color::from_rgba(255, 255, 255, 220);
    let label_y = (label_size as f32) * 0.45;
    let mut draws = vec![Draw::rect(0.0, 0.0, width, height, Color::from_rgba(8, 14, 12, 230))];
    if !full {
        for step in 0..4 {
            let fraction = step as f32 / 3.0;
            let y = plot_top + inner_h * fraction;
            draws.push(path!(
                vec![(left, y), (left + plot_w, y)],
                stroke: 1.0,
                color: if step == 3 { grid_strong } else { grid }
            ));
            let watts = high - scale * f64::from(fraction);
            draws.push(Draw::text(
                4.0,
                (y - label_y).max(0.0),
                axis_watts(watts, grid_step),
                style!(size: label_size, weight: FontWeight::SEMIBOLD, color: Color::from_hex(0xC8_D4_CC), align: TextAlign::Left),
            ));
        }
    }
    if !full {
        for step in 0..5 {
            let fraction = step as f32 / 4.0;
            let x = left + plot_w * fraction;
            draws.push(path!(
                vec![(x, plot_top), (x, plot_bottom)],
                stroke: 1.0,
                color: if step == 0 { grid_strong } else { grid }
            ));
        }
    }
    let marks = [
        (start, left, TextAlign::Left),
        (start + span / 2, left + plot_w * 0.5, TextAlign::Center),
        (now, left + plot_w, TextAlign::Right),
    ];
    let with_day = span >= 12 * 60 * 60;
    for (mark, x, align) in marks {
        draws.push(Draw::text(
            x,
            plot_h + 6.0,
            power_clock(mark, with_day),
            style!(size: label_size, weight: FontWeight::SEMIBOLD, color: Color::from_hex(0xC8_D4_CC), align: align),
        ));
    }
    let mut line = Vec::with_capacity(samples.len().saturating_add(1));
    for (stamp, sample) in samples {
        let x = left + ((*stamp - start) as f32 / span as f32).clamp(0.0, 1.0) * plot_w;
        let y = plot_top + inner_h * (1.0 - ((f64::from(*sample) - low) / scale) as f32);
        line.push((x, y.clamp(plot_top, plot_bottom)));
    }
    if line.len() >= 2 {
        let mut fill_pts = line.clone();
        let first = line[0];
        let last = *line.last().unwrap_or(&first);
        fill_pts.push((last.0, plot_bottom));
        fill_pts.push((first.0, plot_bottom));
        draws.push(fill!(fill_pts, color: Color::from_rgba(0, 204, 102, 70)));
        draws.push(path!(line, stroke: 3.0, color: Color::from_hex(0x00_CC_66)));
    } else if let Some((x, y)) = line.first().copied() {
        draws.push(Draw::circle(x, y, 4.0, Color::from_hex(0x00_CC_66)));
    }
    if full {
        let mut tick = grid_start(low, grid_step);
        let mut count = 0;
        let mut last_label_y: Option<f32> = None;
        while tick <= high + 0.5 && count < 6 {
            count += 1;
            let y = plot_top + inner_h * (1.0 - ((tick - low) / scale) as f32);
            let on_zero = tick.abs() < 0.5;
            draws.push(path!(
                vec![(left, y), (left + plot_w, y)],
                stroke: if on_zero { 3.0 } else { 2.0 },
                color: if on_zero { grid_strong } else { grid }
            ));
            let room = last_label_y.map(|previous| (previous - y).abs() >= label_size as f32).unwrap_or(true);
            if room {
                let mut text_y = (y - label_y).max(0.0);
                if (tick - high).abs() < 1.0 {
                    text_y += 8.0;
                }
                draws.push(Draw::text(
                    left + plot_w + 10.0,
                    text_y,
                    axis_watts(tick, grid_step),
                    style!(size: label_size, weight: FontWeight::BOLD, color: Color::from_hex(0xFF_FF_FF), align: TextAlign::Left),
                ));
                last_label_y = Some(y);
            }
            tick += grid_step;
        }
    }
    if show_hash {
        let orange = Color::from_hex(0xFF_9F_0A);
        let hash_scale = (hash_high - hash_low).max(0.001);
        let mut segment: Vec<(f32, f32)> = Vec::new();
        let mut prev_stamp: Option<i64> = None;
        for (stamp, sample) in hash {
            let gap = prev_stamp.is_some_and(|prev| stamp.saturating_sub(prev) > 20 * 60);
            prev_stamp = Some(*stamp);
            if gap && !segment.is_empty() {
                stroke_orange(&mut draws, &segment, orange);
                segment.clear();
            }
            let x = left + ((*stamp - start) as f32 / span as f32).clamp(0.0, 1.0) * plot_w;
            let y = plot_top + inner_h * (1.0 - ((f64::from(*sample) - hash_low) / hash_scale) as f32);
            segment.push((x, y.clamp(plot_top, plot_bottom)));
        }
        stroke_orange(&mut draws, &segment, orange);
        let hash_size = 22_u32;
        let hash_y = (hash_size as f32) * 0.45;
        for (value, frac) in [
            (hash_high, 0.0_f32),
            ((hash_high + hash_low) * 0.5, 0.5),
            (hash_low, 1.0),
        ] {
            let y = plot_top + inner_h * frac;
            draws.push(Draw::text(
                6.0,
                (y - hash_y).max(plot_top - 4.0),
                format_th_axis(value),
                style!(size: hash_size, weight: FontWeight::BOLD, color: orange, align: TextAlign::Left),
            ));
        }
    }
    if full && !readout.is_empty() {
        draws.push(Draw::text(
            width - 16.0,
            8.0,
            readout,
            style!(size: 56, weight: FontWeight::BOLD, color: Color::from_hex(0x00_CC_66), align: TextAlign::Right, line_height: 1.0),
        ));
    }
    let radius = if full { 0.0 } else { 12.0 };
    canvas(props!(width: width, height: height, border_radius: radius), draws)
}

fn grid_start(low: f64, step: f64) -> f64 {
    let steps = (low / step).ceil();
    if steps.is_finite() { steps * step } else { 0.0 }
}

fn axis_watts(watts: f64, step: f64) -> String {
    let sign = if watts < 0.0 { "-" } else { "" };
    let magnitude = watts.abs();
    if magnitude >= 1000.0 || step >= 1000.0 {
        let kw = magnitude / 1000.0;
        if step >= 1000.0 && step % 1000.0 < 1.0 {
            format!("{sign}{kw:.0} kW")
        } else if step >= 100.0 && (step / 100.0).fract() < 0.01 {
            format!("{sign}{kw:.1} kW")
        } else {
            format!("{sign}{kw:.2} kW")
        }
    } else if magnitude >= 100.0 || step >= 1.0 {
        format!("{sign}{magnitude:.0} W")
    } else {
        format!("{sign}{magnitude:.1} W")
    }
}

fn power_clock(unix: i64, with_day: bool) -> String {
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

fn axis_gutter(label: &str, size: u32) -> f32 {
    (label.chars().count().max(4) as f32) * (size as f32) * 0.58 + 16.0
}

fn span_bar(selected: Span, width: f32, height: f32) -> Node {
    let count = SPANS.len().max(1) as f32;
    let dividers = (count - 1.0) * 2.0;
    let chip_w = ((width - dividers) / count).max(28.0);
    let mut nodes = Vec::new();
    for (index, span) in SPANS.iter().enumerate() {
        if index > 0 {
            nodes.push(canvas(
                props!(width: 2.0, height: height),
                [Draw::rect(0.0, 8.0, 2.0, (height - 16.0).max(8.0), Color::from_rgb(88, 88, 88))],
            ));
        }
        nodes.push(chip_label(span.id(), span.label(), selected == *span, chip_w, height, 22));
    }
    row(props!(width: width, height: height, gap: 0.0), nodes)
}

fn span_row(spans: &[Span], selected: Span, width: f32, height: f32) -> Node {
    let gap = 6.0;
    let count = spans.len().max(1) as f32;
    let chip_w = ((width - gap * (count - 1.0)) / count).max(28.0);
    row(
        props!(width: width, height: height, gap: gap),
        spans
            .iter()
            .copied()
            .map(|span| chip(span.id(), span.label(), selected == span, chip_w, height))
            .collect::<Vec<_>>(),
    )
}

fn key_row(keys: &[(&str, &str)], width: f32) -> Node {
    let gap = 8.0;
    let count = keys.len().max(1) as f32;
    let chip_w = ((width - gap * (count - 1.0)) / count).max(36.0);
    row(
        props!(width: width, height: 40.0, gap: gap),
        keys.iter()
            .copied()
            .map(|(id, label)| chip(id, label, false, chip_w, 40.0))
            .collect::<Vec<_>>(),
    )
}

fn chart_button() -> Node {
    col(
        props!(
            width: 220.0,
            height: 64.0,
            background: Color::from_hex(0x00_CC_66),
            border_radius: 16.0,
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
        ),
        [
            text(
                "Chart",
                style!(
                    size: 34,
                    weight: FontWeight::BOLD,
                    color: Color::from_hex(0x04_12_0C),
                    line_height: 1.0,
                    align: TextAlign::Center,
                ),
            ),
            touchable(
                "power-chart-open",
                props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0),
                Vec::<Draw>::new(),
            ),
        ],
    )
}

fn chip(id: &str, label: &str, on: bool, width: f32, height: f32) -> Node {
    chip_label(id, label, on, width, height, ((height * 0.64) as u32).clamp(20, 40))
}

fn chip_label(id: &str, label: &str, on: bool, width: f32, height: f32, text_size: u32) -> Node {
    let fill = if on { Color::from_hex(0x00_CC_66) } else { Color::from_rgba(255, 255, 255, 28) };
    let ink = if on { Color::from_hex(0x04_12_0C) } else { Color::from_hex(0xFF_FF_FF) };
    col(
        props!(
            width: width,
            height: height,
            background: fill,
            border_radius: (height * 0.22).max(8.0),
            justify_content: Justify::Center,
            cross_align: CrossAlign::Center,
            padding: 2.0,
        ),
        [
            text(
                label,
                style!(
                    size: text_size,
                    weight: FontWeight::BOLD,
                    color: ink,
                    line_height: 1.0,
                    align: TextAlign::Center,
                    valign: VerticalAlign::Center,
                ),
            ),
            touchable(id, props!(inset_top: 0.0, inset_right: 0.0, inset_bottom: 0.0, inset_left: 0.0), Vec::<Draw>::new()),
        ],
    )
}

fn gear_button() -> Node {
    let cx = 20.0;
    let cy = 20.0;
    let ink = Color::from_hex(0xC8_D4_CC);
    let hole = Color::from_hex(0x0C_14_10);
    let mut draws = Vec::new();
    for tooth in 0..8 {
        let angle = tooth as f32 * std::f32::consts::FRAC_PI_4;
        draws.push(Draw::rect(
            cx + angle.cos() * 11.0 - 2.5,
            cy + angle.sin() * 11.0 - 2.5,
            5.0,
            5.0,
            ink,
        ));
    }
    draws.push(Draw::circle(cx, cy, 8.5, ink));
    draws.push(Draw::circle(cx, cy, 3.5, hole));
    touchable("power-gear", props!(width: 40.0, height: 40.0), draws)
}

fn local_offset() -> i64 {
    let now = SystemTime::now();
    bmc_wasm_sdk::format::local_unix_secs_or_system(&now, None) - now.unix_secs
}

fn local_day(unix: i64) -> i32 {
    let local = unix + local_offset();
    strftime(local, "%Y%m%d").parse().unwrap_or(0)
}

fn local_midnight(unix: i64) -> i64 {
    let offset = local_offset();
    let local = unix + offset;
    (local - local.rem_euclid(86_400)) - offset
}

/// Watt-hours since local midnight, from the minute averages plus the fine
/// samples that are still newer than those minutes. Positive watts only.
fn ring_consumption_kwh(ring: &Ring, midnight: i64, interval: i64) -> f64 {
    let interval = interval.max(1);
    let fine_start = ring
        .fine
        .iter()
        .find(|(stamp, _)| *stamp >= midnight)
        .map(|(stamp, _)| *stamp);
    let mut watt_secs = 0.0;
    for (stamp, watts) in &ring.coarse {
        if *stamp < midnight || *watts <= 0.0 {
            continue;
        }
        if fine_start.is_some_and(|start| *stamp >= start.saturating_sub(59)) {
            continue;
        }
        watt_secs += f64::from(*watts) * 60.0;
    }
    let fine = &ring.fine;
    let mut index = fine
        .iter()
        .position(|(stamp, _)| *stamp >= midnight)
        .unwrap_or(fine.len());
    while index < fine.len() {
        let (stamp, watts) = fine[index];
        let next = if index + 1 < fine.len() {
            fine[index + 1].0
        } else {
            stamp.saturating_add(interval)
        };
        let gap = (next - stamp).clamp(1, 120);
        if watts > 0.0 {
            watt_secs += f64::from(watts) * gap as f64;
        }
        index += 1;
    }
    watt_secs / 3_600_000.0
}

fn format_rate(rate: f64) -> String {
    if rate <= 0.0 {
        return String::new();
    }
    let text = format!("{rate:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() { "0".to_owned() } else { trimmed.to_owned() }
}

fn money(value: f64) -> String {
    format!("${:.2}", (value * 100.0).round() / 100.0)
}

/// kWh since local midnight, priced at `rate` dollars per kWh.
fn today_cost_text(kwh: f64, rate: f64) -> String {
    if rate <= 0.0 {
        return format!("{:.1} kWh", kwh.max(0.0));
    }
    let dollars = kwh.max(0.0) * rate;
    // Under $10 a cent takes minutes at a few hundred watts, so the third
    // digit is what shows the total moving with each power sample.
    if dollars < 10.0 {
        format!("${:.3} today", dollars)
    } else {
        format!("{} today", money(dollars))
    }
}

/// What the live watts cost if they held for a day at `rate` dollars per kWh.
fn daily_cost_text(watts: Option<f64>, rate: f64) -> String {
    if rate <= 0.0 {
        return "Set $/kWh".to_owned();
    }
    let Some(watts) = watts else {
        return "—".to_owned();
    };
    let per_day = watts / 1000.0 * rate * 24.0;
    if per_day < 0.0 {
        format!("-${:.2}/day", (per_day.abs() * 100.0).round() / 100.0)
    } else {
        format!("{}/day", money(per_day))
    }
}

fn format_day(day: i32) -> String {
    let text = format!("{day:08}");
    if text.len() != 8 {
        return text;
    }
    format!("{}-{}-{}", &text[0..4], &text[4..6], &text[6..8])
}

fn format_watts(watts: f64) -> String {
    let sign = if watts < 0.0 { "-" } else { "" };
    let magnitude = watts.abs();
    if magnitude >= 1000.0 {
        format!("{sign}{:.3} kW", magnitude / 1000.0)
    } else {
        format!("{sign}{:.1} W", magnitude)
    }
}

fn json_number(raw: &str, key: &str) -> Option<f64> {
    let pattern = format!("\"{key}\"");
    let mut rest = raw;
    while let Some(index) = rest.find(&pattern) {
        let after = rest[index + pattern.len()..].trim_start();
        if let Some(after) = after.strip_prefix(':') {
            let after = after.trim_start();
            let end = after
                .find(|ch: char| !(ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E')))
                .unwrap_or(after.len());
            if let Ok(value) = after[..end].parse::<f64>() {
                if value.is_finite() {
                    return Some(value);
                }
            }
        }
        rest = &rest[index + pattern.len()..];
    }
    None
}

fn json_string(raw: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let index = raw.find(&pattern)?;
    let after = raw[index + pattern.len()..].trim_start().strip_prefix(':')?.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    Some(after[..end].to_owned())
}

fn mqtt_connect(user: &str, pass: &str) -> Vec<u8> {
    // One id for the life of the widget. A new id on every reconnect uses up
    // the 10 MQTT links EcoFlow allows this account per day.
    let client = "hashwatcher-deck";
    let mut body = mqtt_string("MQTT");
    body.extend_from_slice(&[4, 0xC2, 0x00, 45]);
    body.extend(mqtt_string(&client));
    body.extend(mqtt_string(user));
    body.extend(mqtt_string(pass));
    mqtt_packet(0x10, &body)
}

fn mqtt_subscribe(topic: &str, packet_id: u16) -> Vec<u8> {
    let mut body = vec![(packet_id >> 8) as u8, packet_id as u8];
    body.extend(mqtt_string(topic));
    body.push(0);
    mqtt_packet(0x82, &body)
}

fn mqtt_string(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut out = vec![(bytes.len() >> 8) as u8, bytes.len() as u8];
    out.extend_from_slice(bytes);
    out
}

fn mqtt_packet(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend(mqtt_remaining(body.len()));
    out.extend_from_slice(body);
    out
}

fn mqtt_remaining(mut length: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut encoded = (length % 128) as u8;
        length /= 128;
        if length > 0 {
            encoded |= 0x80;
        }
        bytes.push(encoded);
        if length == 0 {
            break;
        }
    }
    bytes
}

fn mqtt_payload(body: &[u8]) -> Option<String> {
    if body.len() < 2 {
        return None;
    }
    let topic_len = (usize::from(body[0]) << 8) | usize::from(body[1]);
    let start = 2 + topic_len;
    if start > body.len() {
        return None;
    }
    String::from_utf8(body[start..].to_vec()).ok()
}

fn take_packet(buf: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    if buf.len() < 2 {
        return None;
    }
    let kind = buf[0];
    let mut value = 0usize;
    let mut multiplier = 1usize;
    let mut index = 1usize;
    loop {
        if index >= buf.len() || multiplier > 128 * 128 * 128 {
            return None;
        }
        let byte = buf[index];
        index += 1;
        value += usize::from(byte & 0x7F) * multiplier;
        if byte & 0x80 == 0 {
            break;
        }
        multiplier *= 128;
    }
    let total = index + value;
    if buf.len() < total || value > 256 * 1024 {
        return None;
    }
    let body = buf[index..total].to_vec();
    buf.drain(..total);
    Some((kind, body))
}

fn b64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let n = (u32::from(data[index]) << 16) | (u32::from(data[index + 1]) << 8) | u32::from(data[index + 2]);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        index += 3;
    }
    if index < data.len() {
        let remain = data.len() - index;
        let mut n = u32::from(data[index]) << 16;
        if remain == 2 {
            n |= u32::from(data[index + 1]) << 8;
        }
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if remain == 2 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push('=');
            out.push('=');
        }
    }
    out
}

fn b64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = text.trim().as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'=' {
            break;
        }
        let b0 = val(*bytes.get(index)?)?;
        let b1 = val(*bytes.get(index + 1)?)?;
        let b2 = bytes.get(index + 2).copied().unwrap_or(b'=');
        let b3 = bytes.get(index + 3).copied().unwrap_or(b'=');
        out.push((b0 << 2) | (b1 >> 4));
        if b2 != b'=' {
            let b2 = val(b2)?;
            out.push((b1 << 4) | (b2 >> 2));
            if b3 != b'=' {
                let b3 = val(b3)?;
                out.push((b2 << 6) | b3);
            }
        }
        index += 4;
    }
    Some(out)
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let digest = hmac_sha256(key, message);
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        key_block[..32].copy_from_slice(&sha256(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = [0u8; 64];
    let mut outer = [0u8; 64];
    for index in 0..64 {
        inner[index] = key_block[index] ^ 0x36;
        outer[index] = key_block[index] ^ 0x5C;
    }
    let mut inner_msg = Vec::with_capacity(64 + message.len());
    inner_msg.extend_from_slice(&inner);
    inner_msg.extend_from_slice(message);
    let inner_hash = sha256(&inner_msg);
    let mut outer_msg = Vec::with_capacity(96);
    outer_msg.extend_from_slice(&outer);
    outer_msg.extend_from_slice(&inner_hash);
    sha256(&outer_msg)
}

fn sha256(message: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut hash: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (message.len() as u64) * 8;
    let mut data = message.to_vec();
    data.push(0x80);
    while (data.len() % 64) != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in data.chunks(64) {
        let mut w = [0u32; 64];
        for index in 0..16 {
            w[index] = u32::from_be_bytes([
                chunk[index * 4],
                chunk[index * 4 + 1],
                chunk[index * 4 + 2],
                chunk[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut a = hash[0];
        let mut b = hash[1];
        let mut c = hash[2];
        let mut d = hash[3];
        let mut e = hash[4];
        let mut f = hash[5];
        let mut g = hash[6];
        let mut h = hash[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[index]).wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        hash[0] = hash[0].wrapping_add(a);
        hash[1] = hash[1].wrapping_add(b);
        hash[2] = hash[2].wrapping_add(c);
        hash[3] = hash[3].wrapping_add(d);
        hash[4] = hash[4].wrapping_add(e);
        hash[5] = hash[5].wrapping_add(f);
        hash[6] = hash[6].wrapping_add(g);
        hash[7] = hash[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (index, word) in hash.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}
