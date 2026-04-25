//! Double-tap modifier hotkey detector.
//!
//! Polls the OS for the trigger modifier (left-Command on macOS, left-Control on Windows)
//! at ~120Hz. When the user double-taps the modifier within 350ms — without any other
//! key being pressed during the sequence — the `on_fire` callback is invoked.
//!
//! The "no other keys pressed" guard relies on `CGEventSourceCounterForEventType` on
//! macOS; on platforms without that primitive the guard is skipped, which is good
//! enough because Ctrl+letter combos rarely produce two clean down→up cycles within
//! the double-tap window.

use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
    fn CGEventSourceCounterForEventType(state_id: i32, event_type: u32) -> u32;
}

#[cfg(target_os = "macos")]
fn read_trigger_modifier_down() -> bool {
    // kVK_Command = 55 (left Cmd). Try HID and combined session sources.
    const KEYCODE_LEFT_CMD: u16 = 55;
    unsafe {
        CGEventSourceKeyState(1, KEYCODE_LEFT_CMD)
            || CGEventSourceKeyState(0, KEYCODE_LEFT_CMD)
    }
}

#[cfg(target_os = "windows")]
fn read_trigger_modifier_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // VK_LCONTROL = 0xA2. High bit set = currently down.
    unsafe { (GetAsyncKeyState(0xA2) as u16 & 0x8000) != 0 }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_trigger_modifier_down() -> bool {
    false
}

/// Counter of non-modifier key-down events since boot. Used to detect whether a stray
/// key was pressed during a candidate double-tap sequence (e.g., Cmd+C), which would
/// invalidate it. `None` on platforms where this primitive isn't available.
#[cfg(target_os = "macos")]
fn read_key_down_counter() -> Option<u32> {
    // kCGEventSourceStateCombinedSessionState = 0, kCGEventKeyDown = 10.
    Some(unsafe { CGEventSourceCounterForEventType(0, 10) })
}

#[cfg(not(target_os = "macos"))]
fn read_key_down_counter() -> Option<u32> {
    None
}

#[derive(Debug, Clone, Copy)]
enum State {
    Idle,
    FirstDown { press_at: Instant, snap: Option<u32> },
    FirstUp { release_at: Instant, snap: Option<u32> },
    SecondDown { press_at: Instant, snap: Option<u32> },
}

const DOUBLE_TAP_WINDOW_MS: u128 = 350;
const MAX_TAP_HOLD_MS: u128 = 300;
const POLL_INTERVAL_MS: u64 = 8;

/// Spawn a background thread that fires `on_fire` on every double-tap of the
/// trigger modifier, gated by `is_enabled`.
pub fn spawn_double_tap_monitor<E, F>(is_enabled: E, on_fire: F)
where
    E: Fn() -> bool + Send + 'static,
    F: Fn() + Send + 'static,
{
    std::thread::spawn(move || {
        let mut state = State::Idle;
        let mut prev_down = false;
        loop {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

            let is_down = read_trigger_modifier_down();
            let now = Instant::now();
            let counter = read_key_down_counter();

            if !is_enabled() {
                state = State::Idle;
                prev_down = is_down;
                continue;
            }

            let counter_changed = |snap: Option<u32>| match (snap, counter) {
                (Some(a), Some(b)) => a != b,
                _ => false,
            };

            state = match state {
                State::Idle => {
                    if is_down && !prev_down {
                        State::FirstDown { press_at: now, snap: counter }
                    } else {
                        State::Idle
                    }
                }
                State::FirstDown { press_at, snap } => {
                    if !is_down && prev_down {
                        if counter_changed(snap)
                            || now.duration_since(press_at).as_millis() > MAX_TAP_HOLD_MS
                        {
                            State::Idle
                        } else {
                            State::FirstUp { release_at: now, snap: counter }
                        }
                    } else if is_down && counter_changed(snap) {
                        State::Idle
                    } else {
                        state
                    }
                }
                State::FirstUp { release_at, snap } => {
                    if now.duration_since(release_at).as_millis() > DOUBLE_TAP_WINDOW_MS {
                        State::Idle
                    } else if counter_changed(snap) {
                        State::Idle
                    } else if is_down && !prev_down {
                        State::SecondDown { press_at: now, snap: counter }
                    } else {
                        state
                    }
                }
                State::SecondDown { press_at, snap } => {
                    if !is_down && prev_down {
                        if counter_changed(snap)
                            || now.duration_since(press_at).as_millis() > MAX_TAP_HOLD_MS
                        {
                            State::Idle
                        } else {
                            on_fire();
                            State::Idle
                        }
                    } else if is_down && counter_changed(snap) {
                        State::Idle
                    } else {
                        state
                    }
                }
            };

            prev_down = is_down;
        }
    });
}
