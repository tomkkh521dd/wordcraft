//! Input as the shell reports it, and the translation into the events egui expects.
//!
//! Coordinates are in vp (density-independent points), the unit ArkUI hands out; egui thinks in
//! points too, so pointer positions pass straight through while `pixels_per_point` handles the
//! scaling when the frame is painted.
//!
//! Key codes are ArkUI's `KeyCode` values (from `@ohos.multimodalInput.keyCode`), mapped here to
//! egui's logical keys so the UI's shortcuts (`crates/ui-egui/src/keys.rs`) work unchanged.

/// What a pointer event did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Down,
    Up,
    Move,
    /// The pointer left the surface (no more hover position).
    Leave,
    /// The gesture was taken away (a system swipe, a second finger turning a drag into a scroll).
    Cancel,
}

impl PointerKind {
    /// The C ABI's numbering; an unknown value is no event at all.
    pub fn from_int(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            2 => Some(Self::Move),
            3 => Some(Self::Leave),
            4 => Some(Self::Cancel),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::Move => "move",
            Self::Leave => "leave",
            Self::Cancel => "cancel",
        }
    }
}

/// Which mouse button (or touch) an event is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Button {
    #[default]
    Primary,
    Secondary,
    Middle,
    /// Touch, or a press with no button: egui treats that as the primary button.
    None,
}

impl Button {
    pub fn from_int(value: i32) -> Self {
        match value {
            0 => Self::Primary,
            1 => Self::Secondary,
            2 => Self::Middle,
            _ => Self::None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Middle => "middle",
            Self::None => "none",
        }
    }

    /// egui's button; a touch or a button-less press is the primary button.
    pub fn egui(self) -> egui::PointerButton {
        match self {
            Self::Primary | Self::None => egui::PointerButton::Primary,
            Self::Secondary => egui::PointerButton::Secondary,
            Self::Middle => egui::PointerButton::Middle,
        }
    }
}

/// Modifier bits as the shell packs them (see `wc_mod_*` in `wc_abi.h`).
pub mod modifiers {
    pub const SHIFT: i32 = 1;
    pub const CTRL: i32 = 2;
    pub const ALT: i32 = 4;
    pub const META: i32 = 8;
    pub const CAPS_LOCK: i32 = 16;

    /// `Shift+Ctrl` for logs and tests; empty for none.
    pub fn describe(bits: i32) -> String {
        let mut parts = Vec::new();
        for (bit, name) in [(CTRL, "Ctrl"), (SHIFT, "Shift"), (ALT, "Alt"), (META, "Meta"), (CAPS_LOCK, "Caps")] {
            if bits & bit != 0 {
                parts.push(name);
            }
        }
        parts.join("+")
    }

    /// egui's view of the same bits. On a HarmonyOS PC the Meta key is the Windows key, so `mac_cmd`
    /// stays off: shortcuts use Ctrl, as in the Windows and Linux builds.
    pub fn egui(bits: i32) -> egui::Modifiers {
        egui::Modifiers { alt: bits & ALT != 0, ctrl: bits & CTRL != 0, shift: bits & SHIFT != 0, mac_cmd: false, command: bits & CTRL != 0 }
    }
}

/// A scroll gesture, in vp, positive down and right (ArkUI's axis sign).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    pub dx: f32,
    pub dy: f32,
}

/// A key press as the shell reports it: the platform key code, plus the character it produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    /// ArkUI's `KeyCode`.
    pub code: i32,
    pub pressed: bool,
    pub repeated: bool,
    pub modifiers: i32,
    /// The character the key produced, when it produced one (`event.keyText`). IME text arrives
    /// through [`crate::App::text`] instead.
    pub text: Option<char>,
}

/// ArkUI's `KeyCode` → egui's logical key. `None` for keys egui has no name for (media, volume,
/// the modifier keys themselves — those travel as [`modifiers`] bits).
pub fn egui_key(code: i32) -> Option<egui::Key> {
    use egui::Key;
    let key = match code {
        // 0–9, then A–Z (ArkUI numbers them in order).
        2000..=2009 => {
            return Some(
                [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9]
                    [(code - 2000) as usize],
            );
        }
        2017..=2042 => {
            const LETTERS: [Key; 26] = [
                Key::A,
                Key::B,
                Key::C,
                Key::D,
                Key::E,
                Key::F,
                Key::G,
                Key::H,
                Key::I,
                Key::J,
                Key::K,
                Key::L,
                Key::M,
                Key::N,
                Key::O,
                Key::P,
                Key::Q,
                Key::R,
                Key::S,
                Key::T,
                Key::U,
                Key::V,
                Key::W,
                Key::X,
                Key::Y,
                Key::Z,
            ];
            return LETTERS.get((code - 2017) as usize).copied();
        }
        2012 => Key::ArrowUp,
        2013 => Key::ArrowDown,
        2014 => Key::ArrowLeft,
        2015 => Key::ArrowRight,
        2016 | 2054 | 2119 => Key::Enter,
        2049 => Key::Tab,
        2050 => Key::Space,
        2055 => Key::Backspace,
        2071 => Key::Delete,
        2083 => Key::Insert,
        2070 => Key::Escape,
        2068 => Key::PageUp,
        2069 => Key::PageDown,
        2081 => Key::Home,
        2082 => Key::End,
        2056 => Key::Backtick,
        2057 => Key::Minus,
        2058 => Key::Equals,
        2059 => Key::OpenBracket,
        2060 => Key::CloseBracket,
        2061 => Key::Backslash,
        2062 => Key::Semicolon,
        2063 => Key::Quote,
        2064 => Key::Slash,
        2043 => Key::Comma,
        2044 => Key::Period,
        2066 => Key::Plus,
        2090..=2101 => {
            const FUNCTION: [Key; 12] =
                [Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12];
            return FUNCTION.get((code - 2090) as usize).copied();
        }
        _ => return None,
    };
    Some(key)
}
