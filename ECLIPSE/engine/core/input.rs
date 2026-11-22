use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyBindings {
    pub move_forward: i32,
    pub move_backward: i32,
    pub move_left: i32,
    pub move_right: i32,
    pub jump: i32,
    pub crouch: i32,
    pub sprint: i32,
    pub escape: i32,
    pub inventory: i32,
    pub debug_overlay: i32,
    pub hotbar_slots: [i32; 8],
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            move_forward: 0x57,
            move_backward: 0x53,
            move_left: 0x41,
            move_right: 0x44,
            jump: 0x20,
            crouch: 0xa2,
            sprint: 0xa0,
            escape: 0x1b,
            inventory: 0x45,
            debug_overlay: 0x72,
            hotbar_slots: std::array::from_fn(|index| 0x31 + index as i32),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeyboardState {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub speed_up: bool,
    pub escape: bool,
    pub inventory: bool,
    pub debug_overlay: bool,
    pub slot_keys: [bool; 8],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseDelta {
    pub x: f32,
    pub y: f32,
}

impl KeyboardState {
    pub fn poll(focused: bool) -> Self {
        Self::poll_with_bindings(focused, KeyBindings::default())
    }

    pub fn poll_with_bindings(focused: bool, bindings: KeyBindings) -> Self {
        if !focused {
            return Self::default();
        }

        Self {
            forward: is_down(bindings.move_forward),
            backward: is_down(bindings.move_backward),
            left: is_down(bindings.move_left),
            right: is_down(bindings.move_right),
            jump: is_down(bindings.jump),
            sneak: is_down(bindings.crouch),
            speed_up: is_down(bindings.sprint),
            escape: is_down(bindings.escape),
            inventory: is_down(bindings.inventory),
            debug_overlay: is_down(bindings.debug_overlay),
            slot_keys: std::array::from_fn(|index| is_down(bindings.hotbar_slots[index])),
        }
    }
}

fn is_down(virtual_key: i32) -> bool {
    unsafe { GetAsyncKeyState(virtual_key) as u16 & 0x8000 != 0 }
}
