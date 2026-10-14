//! Terminal colors, switched off when output is piped or `NO_COLOR` is set.

use owo_colors::OwoColorize;
use std::io::IsTerminal;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

macro_rules! color_fn {
    ($($name:ident),*) => {$(
        pub fn $name(text: impl std::fmt::Display) -> String {
            if enabled() { text.$name().to_string() } else { text.to_string() }
        }
    )*};
}

color_fn!(green, red, yellow, cyan, blue, magenta, dimmed, bold);

/// Colors an HTTP status by class: 2xx green, 3xx cyan, 4xx yellow, 5xx red.
pub fn status(code: u16, text: &str) -> String {
    let label = format!("{code} {text}");
    match code {
        200..=299 => green(label),
        300..=399 => cyan(label),
        400..=499 => yellow(label),
        _ => red(label),
    }
}

/// `1.2 KB`, `830 B`, `3.4 MB`.
pub fn size(bytes: u64) -> String {
    match bytes {
        0..1_024 => format!("{bytes} B"),
        1_024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1_024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// `182 ms`, `1.4 s`.
pub fn duration(ms: f64) -> String {
    if ms < 1_000.0 { format!("{ms:.0} ms") } else { format!("{:.2} s", ms / 1_000.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(size(830), "830 B");
        assert_eq!(size(1_229), "1.2 KB");
        assert_eq!(duration(182.4), "182 ms");
        assert_eq!(duration(1_400.0), "1.40 s");
    }
}
