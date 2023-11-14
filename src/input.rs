use std::collections::HashMap;
use gilrs::Button;
use crate::CURRENT_EMULATOR_STATE;

pub unsafe extern "C" fn libretro_set_input_poll_callback() {
    println!("libretro_set_input_poll_callback")
}

pub unsafe extern "C" fn libretro_set_input_state_callback(port: libc::c_uint, device: libc::c_uint, index: libc::c_uint, id: libc::c_uint) -> i16 {
    // println!("libretro_set_input_state_callback port: {} device: {} index: {} id: {}", port, device, index, id);
    let is_pressed = match &CURRENT_EMULATOR_STATE.buttons_pressed {
        Some(buttons_pressed) => buttons_pressed[id as usize],
        None => 0
    };

    return is_pressed;
}

pub fn setup_joypad_device_map() -> HashMap<Button, usize> {
    return HashMap::from([
        (
            Button::South,
            libretro_sys::DEVICE_ID_JOYPAD_A as usize,
        ),
        (
            Button::East,
            libretro_sys::DEVICE_ID_JOYPAD_B as usize,
        ),
        (
            Button::West,
            libretro_sys::DEVICE_ID_JOYPAD_X as usize,
        ),
        (
            Button::North,
            libretro_sys::DEVICE_ID_JOYPAD_Y as usize,
        ),
        (
            Button::LeftTrigger,
            libretro_sys::DEVICE_ID_JOYPAD_L as usize,
        ),
        (
            Button::LeftTrigger2,
            libretro_sys::DEVICE_ID_JOYPAD_L2 as usize,
        ),
        (
            Button::RightTrigger,
            libretro_sys::DEVICE_ID_JOYPAD_R as usize,
        ),
        (
            Button::RightTrigger2,
            libretro_sys::DEVICE_ID_JOYPAD_R2 as usize,
        ),
        (
            Button::DPadDown,        
            libretro_sys::DEVICE_ID_JOYPAD_DOWN as usize,
        ),
        (
            Button::DPadUp,
            libretro_sys::DEVICE_ID_JOYPAD_UP as usize,
        ),
        (
            Button::DPadRight,
            libretro_sys::DEVICE_ID_JOYPAD_RIGHT as usize,
        ),
        (
            Button::DPadLeft,
            libretro_sys::DEVICE_ID_JOYPAD_LEFT as usize,
        ),
        (
            Button::Start,
            libretro_sys::DEVICE_ID_JOYPAD_START as usize,
        ),
        (
            Button::Select,
            libretro_sys::DEVICE_ID_JOYPAD_SELECT as usize,
        ),
    ]);
}