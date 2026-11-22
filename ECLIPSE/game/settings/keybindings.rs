use eclipse_engine::input::KeyBindings;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlBinding {
    pub action: &'static str,
    pub key: &'static str,
}

pub const KEY_BINDINGS: KeyBindings = KeyBindings {
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
    hotbar_slots: [0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38],
};

pub const CONTROL_BINDINGS: [ControlBinding; 10] = [
    ControlBinding {
        action: "MOVE FORWARD",
        key: "W",
    },
    ControlBinding {
        action: "MOVE BACKWARD",
        key: "S",
    },
    ControlBinding {
        action: "MOVE LEFT",
        key: "A",
    },
    ControlBinding {
        action: "MOVE RIGHT",
        key: "D",
    },
    ControlBinding {
        action: "JUMP",
        key: "SPACE",
    },
    ControlBinding {
        action: "SPRINT / DIVE",
        key: "LEFT SHIFT",
    },
    ControlBinding {
        action: "CROUCH",
        key: "LEFT CTRL",
    },
    ControlBinding {
        action: "OPEN INVENTORY",
        key: "E",
    },
    ControlBinding {
        action: "SELECT HOTBAR",
        key: "1 TO 8",
    },
    ControlBinding {
        action: "MENU OR BACK",
        key: "ESC",
    },
];
