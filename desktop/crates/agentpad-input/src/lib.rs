//! OS key/text/pointer injection. v1: Windows `SendInput` and macOS `CGEvent` only.

#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("agentpad-input v1 supports Windows and macOS only");

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
static AX_CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

static POINTER_STATE: Mutex<PointerState> = Mutex::new(PointerState {
    buttons: 0,
    remainder_x: 0.0,
    remainder_y: 0.0,
});

#[derive(Default)]
struct PointerState {
    buttons: u8,
    remainder_x: f64,
    remainder_y: f64,
}

impl PointerState {
    fn motion(&mut self, dx: f64, dy: f64) -> (i32, i32) {
        fn axis(remainder: &mut f64, delta: f64) -> i32 {
            let total = (*remainder + delta).clamp(i32::MIN as f64, i32::MAX as f64);
            let whole = total.round() as i32;
            *remainder = total - f64::from(whole);
            whole
        }

        (
            axis(&mut self.remainder_x, dx),
            axis(&mut self.remainder_y, dy),
        )
    }
}

#[derive(Debug)]
pub struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("input injection failed")
    }
}

impl std::error::Error for Error {}

pub fn os() -> &'static str {
    #[cfg(windows)]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
}

pub fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        let mut cache = AX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, ok)) = *cache {
            if at.elapsed() < Duration::from_millis(500) {
                return ok;
            }
        }
        let ok = macos::accessibility_trusted();
        *cache = Some((Instant::now(), ok));
        ok
    }
    #[cfg(windows)]
    {
        true
    }
}

pub fn accessibility_debug() -> String {
    #[cfg(target_os = "macos")]
    {
        macos::accessibility_debug()
    }
    #[cfg(windows)]
    {
        "windows".into()
    }
}

pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    macos::open_accessibility_settings();
}

pub fn prompt_accessibility() {
    #[cfg(target_os = "macos")]
    macos::prompt_accessibility();
}

pub fn current_exe() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

pub fn inject_text(text: &str) -> Result<(), Error> {
    if text.is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    return win::inject_text(text);
    #[cfg(target_os = "macos")]
    return macos::inject_text(text);
}

pub fn inject_key(key: &str, modifiers: &[&str]) -> Result<(), Error> {
    let key = normalize_key(key);
    #[cfg(windows)]
    return win::inject_key(key, modifiers);
    #[cfg(target_os = "macos")]
    return macos::inject_key(key, modifiers);
}

pub fn undo() -> Result<(), Error> {
    #[cfg(windows)]
    return inject_key("z", &["Control"]);
    #[cfg(target_os = "macos")]
    return inject_key("z", &["Meta"]);
}

pub fn inject_pointer(dx: f64, dy: f64, buttons: u8, wheel: i32) -> Result<(), Error> {
    let mut state = POINTER_STATE.lock().map_err(|_| Error)?;
    let prev = state.buttons;
    state.buttons = buttons;
    let (dx, dy) = state.motion(dx, dy);
    #[cfg(windows)]
    return win::inject_pointer(dx, dy, prev, buttons, wheel);
    #[cfg(target_os = "macos")]
    return macos::inject_pointer(dx, dy, prev, buttons, wheel);
}

pub fn normalize_key(key: &str) -> &str {
    match key {
        "Esc" | "esc" | "Escape" => "Escape",
        "Return" | "return" | "Enter" | "enter" => "Enter",
        "Tab" | "tab" => "Tab",
        "Backspace" | "backspace" | "Delete" => "Backspace",
        "Space" | "space" => "Space",
        other => other,
    }
}

/// Newly pressed bits and newly released bits.
pub fn button_edges(prev: u8, now: u8) -> (u8, u8) {
    (now & !prev, prev & !now)
}

#[cfg(any(windows, test))]
pub(crate) const POINTER_BURST_GAP: Duration = Duration::from_millis(100);

#[cfg(any(windows, test))]
pub(crate) fn burst_follow(
    prev: Option<(Instant, i32, i32)>,
    now: Instant,
    current: (i32, i32),
    dx: i32,
    dy: i32,
    min: (i32, i32),
    max: (i32, i32),
) -> (i32, i32) {
    let (base_x, base_y) = match prev {
        Some((at, x, y)) if now.duration_since(at) < POINTER_BURST_GAP => (x, y),
        _ => current,
    };
    (
        (base_x + dx).clamp(min.0, max.0),
        (base_y + dy).clamp(min.1, max.1),
    )
}

#[cfg(any(windows, test))]
pub(crate) fn normalize_virtual_abs(
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    vw: i32,
    vh: i32,
) -> (i32, i32) {
    let w = (vw - 1).max(1);
    let h = (vh - 1).max(1);
    let nx = (i64::from(x.clamp(vx, vx + vw.max(1) - 1) - vx) * 65535) / i64::from(w);
    let ny = (i64::from(y.clamp(vy, vy + vh.max(1) - 1) - vy) * 65535) / i64::from(h);
    (nx as i32, ny as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_named_keys() {
        assert_eq!(normalize_key("Esc"), "Escape");
        assert_eq!(normalize_key("Enter"), "Enter");
        assert_eq!(normalize_key("Return"), "Enter");
        assert_eq!(normalize_key("a"), "a");
    }

    #[test]
    fn button_press_release() {
        assert_eq!(button_edges(0, 1), (1, 0));
        assert_eq!(button_edges(1, 0), (0, 1));
        assert_eq!(button_edges(1, 3), (2, 0));
        assert_eq!(button_edges(0, 0), (0, 0));
    }

    #[test]
    fn fractional_round_trip_has_zero_net_motion() {
        let mut state = PointerState::default();
        let mut outward = 0;
        for _ in 0..120 {
            outward += state.motion(0.49, 0.0).0;
        }
        let mut inward = 0;
        for _ in 0..120 {
            inward += state.motion(-0.49, 0.0).0;
        }
        assert_eq!(outward, 59);
        assert_eq!(inward, -59);
        assert_eq!(outward + inward, 0);
    }

    #[test]
    fn burst_round_trip_returns_to_start() {
        let now = Instant::now();
        let first = burst_follow(None, now, (100, 50), 40, -8, (0, 0), (1919, 1079));
        assert_eq!(first, (140, 42));
        let second = burst_follow(
            Some((now, first.0, first.1)),
            now + Duration::from_millis(1),
            (100, 50),
            -40,
            8,
            (0, 0),
            (1919, 1079),
        );
        assert_eq!(second, (100, 50));
    }

    #[test]
    fn virtual_abs_maps_origin_and_far_edge() {
        assert_eq!(normalize_virtual_abs(0, 0, 0, 0, 1920, 1080), (0, 0));
        assert_eq!(
            normalize_virtual_abs(1919, 1079, 0, 0, 1920, 1080),
            (65535, 65535)
        );
        assert_eq!(
            normalize_virtual_abs(100, 50, 0, 0, 1920, 1080),
            (
                (100i64 * 65535 / 1919) as i32,
                (50i64 * 65535 / 1079) as i32
            )
        );
    }
}
