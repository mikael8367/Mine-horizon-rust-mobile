use super::*;
#[test]
fn android_y_is_inverted_and_hats_do_not_erase_key_dpad() {
    let mut d = Devices::default();
    d.axis(7, 1, -1.0);
    d.axis(7, 14, 0.7);
    d.key(7, 19, true);
    d.axis(7, 16, 0.0);
    assert_eq!(d.state().left, [0.0, 1.0]);
    assert_eq!(d.state().right, [0.0, -0.7]);
    assert!(d.state().held(Button::DPadUp));
    d.key(7, 19, false);
    assert!(!d.state().held(Button::DPadUp));
    d.axis(7, 15, 1.0);
    assert!(d.state().held(Button::DPadRight));
}
#[test]
fn a_short_press_survives_until_one_poll_and_disconnect_clears_it() {
    let mut d = Devices::default();
    d.key(4, 96, true);
    assert_eq!(d.active_id(), Some(4));
    d.key(4, 96, false);
    assert!(d.poll().held(Button::South));
    assert!(!d.poll().held(Button::South));
    d.key(4, 96, true);
    d.retain(&[]);
    assert_eq!(d.active_id(), None);
    assert_eq!(d.poll(), PadState::default());
}
#[test]
fn hat_taps_and_direction_changes_survive_one_poll() {
    for (axis, value, button) in [
        (15, -1.0, Button::DPadLeft),
        (15, 1.0, Button::DPadRight),
        (16, -1.0, Button::DPadUp),
        (16, 1.0, Button::DPadDown),
    ] {
        let mut d = Devices::default();
        let mut edges = crate::gamepad::Edges::default();
        d.axis(1, axis, value);
        d.axis(1, axis, 0.0);
        assert_eq!(edges.update(&d.poll(), 0.4), [(button, true)]);
        assert_eq!(edges.update(&d.poll(), 0.4), [(button, false)]);
    }
    let mut d = Devices::default();
    d.axis(1, 15, -1.0);
    d.axis(1, 15, 1.0);
    d.axis(1, 15, 0.0);
    let state = d.poll();
    assert!(state.held(Button::DPadLeft));
    assert!(state.held(Button::DPadRight));
    assert_eq!(d.poll(), PadState::default());

    // A small crossing of the threshold still selects the controller.
    d.axis(1, 15, 0.499);
    d.key(2, 96, true);
    assert!(d.axis(1, 15, 0.501));
    d.axis(1, 15, 0.0);
    assert_eq!(d.active_id(), Some(1));
    assert!(d.poll().held(Button::DPadRight));
}
#[test]
fn removing_one_device_preserves_the_other() {
    let mut d = Devices::default();
    d.key(1, 96, true);
    d.key(2, 97, true);
    d.remove(1);
    assert_eq!(d.active_id(), Some(2));
    assert!(d.poll().held(Button::East));
    d.remove(99);
    assert!(d.state().held(Button::East));
    d.key(1, 96, true);
    d.remove(1);
    assert_eq!(d.active_id(), None);
    // Do not silently switch to another pad with potentially stale input.
    assert_eq!(d.poll(), PadState::default());
    assert!(d.devices.get(&2).unwrap().state().held(Button::East));
    d.key(2, 97, false);
    d.key(2, 97, true);
    assert_eq!(d.active_id(), Some(2));
    assert!(d.poll().held(Button::East));
}
#[test]
fn held_trigger_and_pending_press_do_not_add_together() {
    let mut d = Devices::default();
    d.axis(1, 17, 0.5);
    assert_eq!(d.poll().triggers[0], 0.5);
    assert_eq!(d.poll().triggers[0], 0.5);
    d.axis(1, 17, 0.0);
    assert_eq!(d.poll().triggers[0], 0.0);
    d.axis(1, 17, 0.6);
    d.axis(1, 17, 0.0);
    assert_eq!(d.poll().triggers[0], 0.6);
    assert_eq!(d.poll().triggers[0], 0.0);
}
#[test]
fn triggers_accept_analog_and_digital_reports() {
    let mut d = Devices::default();
    d.key(3, 104, true);
    d.axis(3, 17, 0.0);
    assert!(d.state().held(Button::LeftTrigger));
    assert_eq!(d.state().triggers[0], 1.0);
    d.key(3, 104, false);
    assert_eq!(d.state().triggers[0], 0.0);
    d.axis(3, 23, 0.8);
    assert_eq!(d.state().triggers[0], 0.8);
    d.axis(3, 22, 0.6);
    assert_eq!(d.state().triggers[1], 0.6);
    assert!(!d.key(3, 24, true)); // Volume up is never a game action.
}
#[test]
fn digital_triggers_reach_edges_even_when_tapped_between_polls() {
    let mut d = Devices::default();
    let mut edges = crate::gamepad::Edges::default();
    d.key(1, 105, true);
    d.key(1, 105, false);
    assert_eq!(edges.update(&d.poll(), 0.4), [(Button::RightTrigger, true)]);
    assert_eq!(
        edges.update(&d.poll(), 0.4),
        [(Button::RightTrigger, false)]
    );
}
#[test]
fn drift_neutral_and_repeats_do_not_select_another_controller() {
    let mut d = Devices::default();
    assert!(!d.axis(1, 0, 0.12));
    assert_eq!(d.state(), PadState::default());
    assert!(d.key(2, 96, true));
    assert!(!d.key(2, 96, true));
    d.axis(1, 0, 0.0);
    assert!(d.state().held(Button::South));
    d.retain(&[1]);
    assert_eq!(d.state(), PadState::default());
    d.axis(1, 0, f64::NAN);
    assert_eq!(d.state(), PadState::default());
    assert!(!d.axis(1, 9, 1.0)); // An axis no controller stick uses.
}
#[test]
fn picked_modes_reject_other_devices_and_auto_follows_activity() {
    let mut s = Selection::default();
    assert_eq!((s.mode(), s.active()), (InputMode::Auto, InputMode::Touch));
    assert!(s.configure(InputMode::Touch));
    assert!(!s.configure(InputMode::Touch));
    assert!(!s.note(InputMode::Controller));
    assert_eq!(s.active(), InputMode::Touch);
    assert!(s.configure(InputMode::Controller));
    assert_eq!(s.active(), InputMode::Controller);
    assert!(!s.note(InputMode::Touch));
    assert!(s.configure(InputMode::Auto));
    assert_eq!(s.active(), InputMode::Touch);
    assert!(s.note(InputMode::KeyboardMouse));
    assert_eq!(s.active(), InputMode::KeyboardMouse);
    assert!(s.note(InputMode::Controller));
    assert_eq!(s.active(), InputMode::Controller);
    assert!(!s.note(InputMode::Auto));
    assert_eq!(s.active(), InputMode::Controller);
}
#[test]
fn input_modes_save_by_name() {
    let names: Vec<_> = InputMode::ALL
        .iter()
        .map(|m| serde_json::to_string(m).unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "\"auto\"",
            "\"touch\"",
            "\"keyboard-mouse\"",
            "\"controller\""
        ]
    );
    assert!(InputMode::ALL.iter().all(|m| !m.label().is_empty()));
}

#[test]
fn shared_button_codes_release_only_after_the_last_key() {
    for (first, second) in [(4, 97), (97, 4)] {
        let mut d = Devices::default();
        let mut edges = crate::gamepad::Edges::default();
        assert!(d.key(1, first, true));
        assert_eq!(edges.update(&d.poll(), 0.4), [(Button::East, true)]);
        assert!(!d.key(1, second, true));
        assert!(!d.key(1, first, true));
        assert!(edges.update(&d.poll(), 0.4).is_empty());
        assert!(!d.key(1, first, false));
        assert!(!d.key(1, first, false));
        assert!(d.state().held(Button::East));
        assert!(edges.update(&d.poll(), 0.4).is_empty());
        assert!(!d.key(1, second, false));
        assert_eq!(edges.update(&d.poll(), 0.4), [(Button::East, false)]);
        assert!(d.key(1, second, true));
    }
}

#[test]
fn shared_codes_keep_short_press_and_reset_per_device() {
    let mut d = Devices::default();
    assert!(d.key(1, 4, true));
    assert!(!d.key(1, 97, true));
    assert!(!d.key(1, 4, false));
    assert!(!d.key(1, 97, false));
    assert!(d.poll().held(Button::East));
    assert!(!d.poll().held(Button::East));
    assert!(d.key(1, 4, true));
    assert!(d.key(2, 97, true));
    d.remove(1);
    assert!(d.state().held(Button::East));
    d.clear();
    assert!(d.key(1, 4, true));
}

