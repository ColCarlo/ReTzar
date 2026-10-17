//! Per-frame input facts that `ButtonInput` alone loses: the position of a
//! press when a fast drag ends in the same frame, and modifiers held at the
//! moment a group key was pressed.
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::{PrimaryWindow, WindowEvent},
};

#[derive(Resource, Default)]
pub struct PointerFrame {
    pub cursor: Option<Vec2>,
    pub press: Option<Vec2>,
}

pub const GROUP_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

#[derive(Resource, Default)]
pub struct KeyboardChordFrame {
    pub control: [bool; 2],
    /// Enter pressed with Alt held in this frame: the window's full screen,
    /// not the Enter of the game.
    pub alt_enter: bool,
    pub groups: [Option<bool>; 9],
    pub zero: Option<bool>,
}
impl KeyboardChordFrame {
    pub fn record(&mut self, event: &KeyboardInput) {
        match event.key_code {
            KeyCode::ControlLeft => self.control[0] = event.state == ButtonState::Pressed,
            KeyCode::ControlRight => self.control[1] = event.state == ButtonState::Pressed,
            key if event.state == ButtonState::Pressed && !event.repeat => {
                if key == KeyCode::Digit0 {
                    self.zero = Some(self.control.iter().any(|&down| down));
                }
                if let Some(i) = GROUP_KEYS.iter().position(|&k| k == key) {
                    // Remember modifiers at the key press, even if released in this frame.
                    self.groups[i] = Some(self.control.iter().any(|&down| down));
                }
            }
            _ => {}
        }
    }
}

pub fn pointer_events(
    mut events: MessageReader<WindowEvent>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut pointer: ResMut<PointerFrame>,
) {
    pointer.press = None;
    let Ok((window, state)) = windows.single() else {
        return;
    };
    for event in events.read() {
        match event {
            WindowEvent::CursorMoved(e) if e.window == window => pointer.cursor = Some(e.position),
            WindowEvent::CursorLeft(e) if e.window == window => pointer.cursor = None,
            WindowEvent::MouseButtonInput(e)
                if e.window == window
                    && e.button == MouseButton::Left
                    && e.state == ButtonState::Pressed =>
            {
                // Preserve the press position when a fast drag ends in the same frame.
                pointer.press = pointer.cursor.or_else(|| state.cursor_position());
            }
            _ => {}
        }
    }
}

fn navigation_key(key: &Key) -> Option<KeyCode> {
    Some(match key {
        Key::ArrowLeft => KeyCode::ArrowLeft,
        Key::ArrowRight => KeyCode::ArrowRight,
        Key::ArrowUp => KeyCode::ArrowUp,
        Key::ArrowDown => KeyCode::ArrowDown,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        _ => return None,
    })
}

/// Navigation keys follow the logical key, so layouts that remap them work.
pub fn logical_navigation(
    mut events: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut chords: ResMut<KeyboardChordFrame>,
    mut typed: ResMut<TypedKeys>,
) {
    chords.groups = [None; 9];
    chords.zero = None;
    chords.alt_enter = false;
    typed.pressed.clear();
    // Alt as the window has it, let go when the window lost the focus.
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    for event in events.read() {
        chords.record(event);
        if alt
            && event.state == ButtonState::Pressed
            && !event.repeat
            && matches!(event.key_code, KeyCode::Enter | KeyCode::NumpadEnter)
        {
            chords.alt_enter = true;
        }
        typed.record(event);
        if let Some(key) = navigation_key(&event.logical_key) {
            if event.state == ButtonState::Pressed {
                keys.press(key);
            } else {
                keys.release(key);
            }
        }
    }
    // Alt+Enter is taken by the window: the game does not see that Enter.
    if chords.alt_enter {
        keys.clear_just_pressed(KeyCode::Enter);
        keys.clear_just_pressed(KeyCode::NumpadEnter);
    }
}

/// Characters held down, by the physical key that typed them: the keys bound
/// as `char:x` follow the keyboard layout, as the original's virtual keys.
#[derive(Resource, Default)]
pub struct TypedKeys {
    held: std::collections::HashMap<KeyCode, String>,
    pressed: std::collections::HashSet<String>,
}
impl TypedKeys {
    /// A character key held down, for the tests.
    #[cfg(test)]
    pub fn hold(&mut self, key: KeyCode, text: &str) {
        self.held.insert(key, text.into());
    }
    /// The character key let go, for the tests.
    #[cfg(test)]
    pub fn release(&mut self, key: KeyCode) {
        self.held.remove(&key);
    }
    fn record(&mut self, event: &KeyboardInput) {
        match event.state {
            ButtonState::Pressed => {
                if let Key::Character(c) = &event.logical_key {
                    let c = c.to_lowercase();
                    if !event.repeat {
                        self.pressed.insert(c.clone());
                    }
                    self.held.insert(event.key_code, c);
                }
            }
            ButtonState::Released => {
                self.held.remove(&event.key_code);
            }
        }
    }
}

/// Every physical key a command can be bound to, by its name.
fn key_codes() -> &'static std::collections::HashMap<String, KeyCode> {
    static TABLE: std::sync::OnceLock<std::collections::HashMap<String, KeyCode>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        use KeyCode::*;
        [
            KeyA,
            KeyB,
            KeyC,
            KeyD,
            KeyE,
            KeyF,
            KeyG,
            KeyH,
            KeyI,
            KeyJ,
            KeyK,
            KeyL,
            KeyM,
            KeyN,
            KeyO,
            KeyP,
            KeyQ,
            KeyR,
            KeyS,
            KeyT,
            KeyU,
            KeyV,
            KeyW,
            KeyX,
            KeyY,
            KeyZ,
            Digit0,
            Digit1,
            Digit2,
            Digit3,
            Digit4,
            Digit5,
            Digit6,
            Digit7,
            Digit8,
            Digit9,
            F1,
            F2,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            F10,
            F11,
            F12,
            ArrowLeft,
            ArrowRight,
            ArrowUp,
            ArrowDown,
            Home,
            End,
            PageUp,
            PageDown,
            Insert,
            Delete,
            Backspace,
            Tab,
            Enter,
            Space,
            Numpad0,
            Numpad1,
            Numpad2,
            Numpad3,
            Numpad4,
            Numpad5,
            Numpad6,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadAdd,
            NumpadSubtract,
            NumpadMultiply,
            NumpadDivide,
            NumpadDecimal,
            NumpadEnter,
            Pause,
            ScrollLock,
        ]
        .into_iter()
        .map(|k| (format!("{k:?}"), k))
        .collect()
    })
}

/// The name under which a key press is bound, or `None` for keys that
/// cannot carry a command (modifiers, Escape).
pub fn key_name(event: &KeyboardInput) -> Option<String> {
    if let Key::Character(c) = &event.logical_key {
        let c = c.to_lowercase();
        if c.chars().count() == 1 && !c.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(format!("char:{c}"));
        }
    }
    let name = format!("{:?}", event.key_code);
    key_codes().contains_key(&name).then_some(name)
}

/// Whether the key bound to `name` is held.
pub fn held(keys: &ButtonInput<KeyCode>, typed: &TypedKeys, name: &str) -> bool {
    match name.strip_prefix("char:") {
        Some(c) => typed.held.values().any(|v| v == c),
        None => key_codes().get(name).is_some_and(|&k| keys.pressed(k)),
    }
}
/// Whether the key bound to `name` went down in this frame.
pub fn hit(keys: &ButtonInput<KeyCode>, typed: &TypedKeys, name: &str) -> bool {
    match name.strip_prefix("char:") {
        Some(c) => typed.pressed.contains(c),
        None => key_codes().get(name).is_some_and(|&k| keys.just_pressed(k)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn keyboard_event(key: KeyCode, state: ButtonState) -> KeyboardInput {
        KeyboardInput {
            key_code: key,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }
    #[test]
    fn group_shortcuts_keep_modifier_order_within_a_frame() {
        let mut frame = KeyboardChordFrame::default();
        for (key, state) in [
            (KeyCode::ControlLeft, ButtonState::Pressed),
            (KeyCode::Digit1, ButtonState::Pressed),
            (KeyCode::Digit1, ButtonState::Released),
            (KeyCode::ControlLeft, ButtonState::Released),
            (KeyCode::Digit2, ButtonState::Pressed),
        ] {
            frame.record(&keyboard_event(key, state));
        }
        assert_eq!(frame.groups[0], Some(true));
        assert_eq!(frame.groups[1], Some(false));
        assert_eq!(frame.control, [false, false]);
        frame.record(&keyboard_event(KeyCode::ControlRight, ButtonState::Pressed));
        frame.groups = [None; 9];
        frame.record(&keyboard_event(KeyCode::Digit9, ButtonState::Pressed));
        assert_eq!(frame.groups[8], Some(true));
    }
}
