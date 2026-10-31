//! Display strings shared by the Deck widget and the unit tests.

pub fn format_hashrate(ths: Option<f64>) -> String {
    match ths {
        Some(value) if value >= 10.0 => format!("{:.0} TH/s", value),
        Some(value) if value >= 1.0 => format!("{:.1} TH/s", value),
        Some(value) if value > 0.0 => format!("{:.0} GH/s", value * 1_000.0),
        _ => "—".to_owned(),
    }
}

pub fn format_power(watts: Option<f64>) -> String {
    match watts {
        Some(value) if value >= 1000.0 => format!("{:.2} kW", value / 1000.0),
        Some(value) if value > 0.0 => format!("{:.0} W", value),
        _ => "—".to_owned(),
    }
}

pub fn format_temp(celsius: Option<f64>) -> String {
    match celsius {
        Some(value) if value > 0.0 => format!("{:.0}°C", value),
        _ => "—".to_owned(),
    }
}

pub fn format_uptime(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds.filter(|value| *value > 0) else {
        return "—".to_owned();
    };
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

pub fn format_efficiency(joules_per_terahash: Option<f64>) -> String {
    match joules_per_terahash {
        Some(value) if value > 0.0 && value < 10_000.0 => format!("{:.1} J/TH", value),
        _ => "—".to_owned(),
    }
}
