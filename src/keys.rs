//! Remappable keys of the map. Defaults are the original's (HELP\HK.TXT and
//! the key handler 0x4f515a); the player can change them in the options.
//!
//! A key is stored by name: a physical key as the front end names it
//! (`ArrowLeft`, `F2`, `NumpadAdd`, `KeyA`, …) or a typed character as
//! `char:x`. Punctuation keys are characters because the original read
//! Windows virtual keys, which follow the keyboard layout: its health-bar
//! key VK_OEM_3 is «`» on a US keyboard and «ò» on an Italian one.

/// Commands of the map that the player can bind, with their Italian label.
pub const ACTIONS: [(&str, &str); 28] = [
    ("scroll_left", "Scorri a sinistra"),
    ("scroll_right", "Scorri a destra"),
    ("scroll_up", "Scorri in alto"),
    ("scroll_down", "Scorri in basso"),
    ("center", "Centra la selezione"),
    ("next_in_group", "Unità successiva del gruppo"),
    ("event_last", "Ultimo evento"),
    ("event_previous", "Evento precedente"),
    ("event_next", "Evento successivo"),
    ("destination", "Destinazione o raduno"),
    ("health_bars", "Barre della salute"),
    ("transparent_trees", "Alberi trasparenti"),
    ("delete", "Elimina la selezione"),
    ("speed_up", "Aumenta la velocità"),
    ("speed_down", "Riduci la velocità"),
    ("quick_save", "Salvataggio rapido"),
    ("quick_load", "Caricamento rapido"),
    ("score", "Punteggio e tempo"),
    ("inventory_previous", "Pagina precedente dell'inventario"),
    ("inventory_next", "Pagina seguente dell'inventario"),
    ("help", "Aiuto"),
    ("diplomacy", "Diplomazia"),
    ("quests", "Obiettivi"),
    ("options", "Opzioni"),
    ("pause", "Menu della partita"),
    ("chat", "Conversazione"),
    ("tribute", "Tributi"),
    // The cheat panel of this version, while the game's options switch it on.
    ("cheats", "Pannello dei trucchi"),
];

/// The original key of a command.
pub fn default_key(action: &str) -> String {
    match action {
        "scroll_left" => "ArrowLeft".into(),
        "scroll_right" => "ArrowRight".into(),
        "scroll_up" => "ArrowUp".into(),
        "scroll_down" => "ArrowDown".into(),
        "center" => "Space".into(),
        "next_in_group" => "Tab".into(),
        "event_last" => "Home".into(),
        "event_previous" => "PageUp".into(),
        "event_next" => "PageDown".into(),
        "destination" => "End".into(),
        // VK_OEM_3 (0x4f641e) and VK_OEM_5, as the current layout types them.
        "health_bars" => format!("char:{}", oem_char(0xc0).unwrap_or('`')),
        "transparent_trees" => format!("char:{}", oem_char(0xdc).unwrap_or('\\')),
        "delete" => "Delete".into(),
        // Only the numeric keypad changes the speed (0x4f58ce, 0x4f592c).
        "speed_up" => "NumpadAdd".into(),
        "speed_down" => "NumpadSubtract".into(),
        "quick_save" => "F2".into(),
        "quick_load" => "F3".into(),
        "score" => "F4".into(),
        "inventory_previous" => "char:,".into(),
        "inventory_next" => "char:.".into(),
        "help" => "F1".into(),
        "diplomacy" => "F5".into(),
        "tribute" => "F6".into(),
        // F7 alone is free in the original (with Ctrl or Shift a bookmark).
        "cheats" => "F7".into(),
        "quests" => "F8".into(),
        "options" => "F9".into(),
        "pause" => "F10".into(),
        "chat" => "Enter".into(),
        _ => String::new(),
    }
}

/// The character a virtual key types with the current keyboard layout.
fn oem_char(vk: u32) -> Option<char> {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MapVirtualKeyW(code: u32, map: u32) -> u32;
        }
        // MAPVK_VK_TO_CHAR; dead keys set the high bit. Read-only query.
        let c = unsafe { MapVirtualKeyW(vk, 2) } & 0x7fff_ffff;
        char::from_u32(c)
            .filter(|c| !c.is_control())
            .map(|c| c.to_lowercase().next().unwrap_or(c))
    }
    #[cfg(not(windows))]
    {
        let _ = vk;
        None
    }
}

/// A key name as the options page shows it.
pub fn label(name: &str) -> String {
    if let Some(c) = name.strip_prefix("char:") {
        return match c {
            " " => "Spazio".into(),
            c => c.to_uppercase(),
        };
    }
    if let Some(letter) = name.strip_prefix("Key") {
        return letter.into();
    }
    if let Some(digit) = name.strip_prefix("Digit") {
        return digit.into();
    }
    match name {
        "" => "—".into(),
        "ArrowLeft" => "Freccia sinistra".into(),
        "ArrowRight" => "Freccia destra".into(),
        "ArrowUp" => "Freccia su".into(),
        "ArrowDown" => "Freccia giù".into(),
        "Space" => "Spazio".into(),
        "Enter" => "Invio".into(),
        "Tab" => "Tab".into(),
        "Delete" => "Canc".into(),
        "Insert" => "Ins".into(),
        // «Indietro» is also the command grid's arrow, «Back» in English:
        // the other languages name the key on its own.
        "Backspace" if !crate::lang::italian() => crate::lang::t("Tasto Indietro").into(),
        "Backspace" => "Indietro".into(),
        "Home" => "Home".into(),
        "End" => "Fine".into(),
        "PageUp" => "Pag su".into(),
        "PageDown" => "Pag giù".into(),
        "NumpadAdd" => "+ (tastierino)".into(),
        "NumpadSubtract" => "- (tastierino)".into(),
        "NumpadMultiply" => "* (tastierino)".into(),
        "NumpadDivide" => "/ (tastierino)".into(),
        "NumpadEnter" => "Invio (tastierino)".into(),
        other => other.strip_prefix("Numpad").map_or_else(
            || other.to_owned(),
            |n| crate::lang::fill("{} (tastierino)", &[&n]),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_action_has_a_distinct_default() {
        let keys: Vec<_> = ACTIONS.iter().map(|(a, _)| default_key(a)).collect();
        assert!(keys.iter().all(|k| !k.is_empty()));
        for (i, k) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(k), "{k} bound twice");
        }
        assert_eq!(label("ArrowLeft"), "Freccia sinistra");
        assert_eq!(label("char:ò"), "Ò");
        assert_eq!(label("KeyW"), "W");
    }
}
