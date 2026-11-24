// Adapted from Starling (https://github.com/starling-client/starling, `android` branch at
// 150a414b09a1, `crates/horizon-client-render/src/android_gamepad.rs` and the input-mode part of
// `android_runtime.rs`), written by Vasysik and the Starling contributors. Copyright (c) 2026
// Ludovic Marie and the Horizon contributors, under the same MIT license as Horizon (LICENSE at
// the repository root).

//! Controllers as Android reports them: its standard key codes and motion axes,
//! the same for USB and Bluetooth pads, decoded into `gamepad::PadState`, and
//! which input device (touch, keyboard and mouse, or a controller) is in charge.
//! Pure logic: the Android event backend feeds it, nothing here talks to the OS.
use crate::gamepad::{Button, PadState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The input device the player picked in the options, or the one in use.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode {
    /// Follow whichever device was used last.
    #[default]
    Auto,
    Touch,
    KeyboardMouse,
    Controller,
}
impl InputMode {
    pub const ALL: [Self; 4] = [
        Self::Auto,
        Self::Touch,
        Self::KeyboardMouse,
        Self::Controller,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Touch => "Touch & Android keyboard",
            Self::KeyboardMouse => "Keyboard & mouse",
            Self::Controller => "Controller",
        }
    }
}

// Android `MotionEvent` axes (AXIS_*).
const AXIS_X: usize = 0;
const AXIS_Y: usize = 1;
const AXIS_Z: usize = 11;
const AXIS_RZ: usize = 14;
const AXIS_HAT_X: usize = 15;
const AXIS_HAT_Y: usize = 16;
const AXIS_LTRIGGER: usize = 17;
const AXIS_RTRIGGER: usize = 18;
const AXIS_GAS: usize = 22;
const AXIS_BRAKE: usize = 23;
const AXES: usize = 24;

/// The Android `KeyEvent` key code (KEYCODE_*) of a controller button. Back
/// (4) is East, as on Android's own screens; volume and other system keys are
/// never game input.
pub fn button(code: u32) -> Option<Button> {
    Some(match code {
        96 => Button::South,
        97 | 4 => Button::East,
        99 => Button::West,
        100 => Button::North,
        102 => Button::LeftBumper,
        103 => Button::RightBumper,
        104 => Button::LeftTrigger,
        105 => Button::RightTrigger,
        106 => Button::LeftStick,
        107 => Button::RightStick,
        108 => Button::Start,
        109 => Button::Select,
        19 => Button::DPadUp,
        20 => Button::DPadDown,
        21 => Button::DPadLeft,
        22 => Button::DPadRight,
        _ => return None,
    })
}

/// One controller: its key states, its last axis values, and presses too short
/// to be seen by a poll (released before the next frame).
#[derive(Default)]
struct Device {
    keys: PadState,
    axes: [f32; AXES],
    pending: PadState,
}
impl Device {
    fn state(&self) -> PadState {
        let a = &self.axes;
        let mut s = self.keys;
        // Android's Y axes grow downwards; `PadState`'s grow upwards.
        s.left = [a[AXIS_X], -a[AXIS_Y]];
        s.right = [a[AXIS_Z], -a[AXIS_RZ]];
        // Some pads report triggers as LTRIGGER/RTRIGGER, others as BRAKE/GAS,
        // others only as L2/R2 keys: `Edges` reads triggers from `triggers`.
        let digital = |b| if s.held(b) { 1.0 } else { 0.0 };
        let triggers = [
            a[AXIS_LTRIGGER]
                .max(a[AXIS_BRAKE])
                .max(digital(Button::LeftTrigger)),
            a[AXIS_RTRIGGER]
                .max(a[AXIS_GAS])
                .max(digital(Button::RightTrigger)),
        ];
        s.triggers = triggers;
        // A hat D-pad adds to a key D-pad; a centred hat doesn't release keys.
        for (button, held) in [
            (Button::DPadLeft, a[AXIS_HAT_X] < -0.5),
            (Button::DPadRight, a[AXIS_HAT_X] > 0.5),
            (Button::DPadUp, a[AXIS_HAT_Y] < -0.5),
            (Button::DPadDown, a[AXIS_HAT_Y] > 0.5),
        ] {
            s.set(button, held || s.held(button));
        }
        s
    }
}

/// The connected controllers by Android device id, and the one in use.
#[derive(Default)]
pub struct Devices {
    devices: BTreeMap<i32, Device>,
    active: Option<i32>,
}
impl Devices {
    /// A motion event. Returns true only for a meaningful change, so a resting
    /// or drifting stick never takes over from a finger or a mouse.
    pub fn axis(&mut self, id: i32, axis: u32, value: f64) -> bool {
        let index = axis as usize;
        if ![
            AXIS_X,
            AXIS_Y,
            AXIS_Z,
            AXIS_RZ,
            AXIS_HAT_X,
            AXIS_HAT_Y,
            AXIS_LTRIGGER,
            AXIS_RTRIGGER,
            AXIS_GAS,
            AXIS_BRAKE,
        ]
        .contains(&index)
        {
            return false;
        }
        let value = if value.is_finite() {
            value.clamp(-1.0, 1.0) as f32
        } else {
            0.0
        };
        let d = self.devices.entry(id).or_default();
        let old = std::mem::replace(&mut d.axes[index], value);
        // Latch hat direction entries, including a direct change of direction,
        // so a tap released before the next poll is not lost.
        let hat_buttons = match index {
            AXIS_HAT_X => Some((Button::DPadLeft, Button::DPadRight)),
            AXIS_HAT_Y => Some((Button::DPadUp, Button::DPadDown)),
            _ => None,
        };
        let mut hat_press = false;
        if let Some((negative, positive)) = hat_buttons {
            if old >= -0.5 && value < -0.5 {
                d.pending.set(negative, true);
                hat_press = true;
            }
            if old <= 0.5 && value > 0.5 {
                d.pending.set(positive, true);
                hat_press = true;
            }
        }
        if value >= 0.5 && old < 0.5 {
            let trigger = match index {
                AXIS_LTRIGGER | AXIS_BRAKE => Some(0),
                AXIS_RTRIGGER | AXIS_GAS => Some(1),
                _ => None,
            };
            if let Some(i) = trigger {
                d.pending.triggers[i] = value;
            }
        }
        let activity = hat_press || (value.abs() > 0.25 && (value - old).abs() > 0.01);
        if activity {
            self.active = Some(id);
        }
        activity
    }
    /// A key event. Returns true for a new press of a controller button (key
    /// repeats and releases are not activity).
    pub fn key(&mut self, id: i32, code: u32, down: bool) -> bool {
        let Some(b) = button(code) else { return false };
        let d = self.devices.entry(id).or_default();
        let activity = down && !d.keys.held(b);
        d.keys.set(b, down);
        if activity {
            d.pending.set(b, true);
            match b {
                Button::LeftTrigger => d.pending.triggers[0] = 1.0,
                Button::RightTrigger => d.pending.triggers[1] = 1.0,
                _ => {}
            }
            self.active = Some(id);
        }
        activity
    }
    /// Forget one disconnected controller without disturbing the others.
    pub fn remove(&mut self, id: i32) {
        self.devices.remove(&id);
        if self.active == Some(id) {
            self.active = None;
        }
    }
    /// Forget controllers that are no longer connected.
    pub fn retain(&mut self, ids: &[i32]) {
        self.devices.retain(|id, _| ids.contains(id));
        if self.active.is_some_and(|id| !ids.contains(&id)) {
            self.active = None;
        }
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn active_id(&self) -> Option<i32> {
        self.active
    }
    /// The controller in use, without consuming short presses.
    pub fn state(&self) -> PadState {
        self.active
            .and_then(|id| self.devices.get(&id))
            .map(Device::state)
            .unwrap_or_default()
    }
    /// The controller in use, once per frame: a press released since the last
    /// poll still shows as held this once.
    pub fn poll(&mut self) -> PadState {
        self.active
            .and_then(|id| self.devices.get_mut(&id))
            .map(|d| {
                let state = d.state();
                let pending = std::mem::take(&mut d.pending);
                let mut result = state.merge(&pending);
                // Pending input belongs to the same pad, not a second source.
                // Adding trigger values here would falsely amplify a held trigger.
                for i in 0..2 {
                    result.triggers[i] = state.triggers[i].max(pending.triggers[i]);
                }
                result
            })
            .unwrap_or_default()
    }
}

/// The picked input mode and the device in charge. In `Auto` the last device
/// used takes over; a picked mode ignores the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    mode: InputMode,
    active: InputMode,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            mode: InputMode::Auto,
            active: InputMode::Touch,
        }
    }
}
impl Selection {
    /// The mode picked in the options.
    pub fn mode(self) -> InputMode {
        self.mode
    }
    /// The device in charge now; never `Auto`.
    pub fn active(self) -> InputMode {
        self.active
    }
    /// Pick a mode. Returns true when it changed: the caller drops held
    /// controller and keypad input so nothing stays pressed across the switch.
    pub fn configure(&mut self, mode: InputMode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.active = if mode == InputMode::Auto {
            InputMode::Touch
        } else {
            mode
        };
        true
    }
    /// A device of `kind` was used. Returns whether it may act: false when
    /// another mode was picked.
    pub fn note(&mut self, kind: InputMode) -> bool {
        if kind == InputMode::Auto || (self.mode != InputMode::Auto && self.mode != kind) {
            return false;
        }
        self.active = kind;
        true
    }
}

#[cfg(test)]
mod tests;
