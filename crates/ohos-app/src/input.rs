//! Input as the shell reports it: everything by value, so nothing here needs pointers.
//!
//! Coordinates are in vp (density-independent points), the unit ArkUI hands out; the app scales
//! them by the display density when it draws. Button and pointer kinds are small integers on the
//! C ABI, matching `wc_abi.h`.

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
    /// Touch, or a press with no button.
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
    /// The platform's key code (`KeyCode` in ArkTS), passed through unmapped for now.
    pub code: i32,
    pub pressed: bool,
    pub repeated: bool,
    pub modifiers: i32,
    /// The character the key produced, when it produced one (`event.keyText`). IME text arrives
    /// through [`crate::App::text`] instead.
    pub text: Option<char>,
}
