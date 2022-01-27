use crate::coord::Coord;
use crate::logging::log;
use interception::{is_mouse, Device, Filter, Interception, MouseFlags, MouseState, Stroke};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::PWSTR,
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetKeyNameTextW, GetKeyboardState, MapVirtualKeyW, VK_LBUTTON,
            VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
        },
        WindowsAndMessaging::MAPVK_VK_TO_VSC_EX,
    },
};

const INTERCEPTION_ERR: &str = "Error initializing interception - is the interception driver installed? (https://github.com/oblitum/Interception)";

trait Empty {
    fn default() -> Self;
}
impl Empty for Stroke {
    fn default() -> Self {
        Stroke::Mouse {
            state: MouseState::empty(),
            flags: MouseFlags::empty(),
            rolling: 0,
            x: 0,
            y: 0,
            information: 0,
        }
    }
}

trait CaptureMouse {
    fn capture_mouse(&mut self) -> i32;
}
impl CaptureMouse for Interception {
    fn capture_mouse(&mut self) -> i32 {
        log!("Looking for mouse...");
        self.set_filter(is_mouse, Filter::MouseFilter(MouseState::all()));
        let mouse_dev = self.wait();
        self.set_filter(is_mouse, Filter::MouseFilter(MouseState::empty()));
        log!("Found mouse");
        mouse_dev
    }
}

pub struct InterceptionState {
    interception: Interception,
    mouse_dev: Device,
    click_down: MouseState,
    click_up: MouseState,
}

impl InterceptionState {
    pub fn new(mouse_dev: Device) -> Result<Self, &'static str> {
        let interception = Interception::new().ok_or(INTERCEPTION_ERR)?;

        Ok(InterceptionState {
            interception,
            mouse_dev,
            click_down: MouseState::LEFT_BUTTON_DOWN,
            click_up: MouseState::LEFT_BUTTON_UP,
        })
    }

    pub fn click_down(&self) {
        let mut stroke = Stroke::default();
        if let Stroke::Mouse { ref mut state, .. } = stroke {
            *state = self.click_down;
        }
        self.interception.send(self.mouse_dev, &[stroke]);
    }

    pub fn click_up(&self) {
        let mut stroke = Stroke::default();
        if let Stroke::Mouse { ref mut state, .. } = stroke {
            *state = self.click_up;
        }
        self.interception.send(self.mouse_dev, &[stroke]);
    }

    pub fn set_click_keycode(&mut self, keycode: u16) -> Result<(), &'static str> {
        let (click_down, click_up) = match keycode.into() {
            VK_LBUTTON => (MouseState::LEFT_BUTTON_DOWN, MouseState::LEFT_BUTTON_UP),
            VK_RBUTTON => (MouseState::RIGHT_BUTTON_DOWN, MouseState::RIGHT_BUTTON_UP),
            VK_MBUTTON => (MouseState::MIDDLE_BUTTON_DOWN, MouseState::MIDDLE_BUTTON_UP),
            VK_XBUTTON1 => (MouseState::BUTTON_4_DOWN, MouseState::BUTTON_4_UP),
            VK_XBUTTON2 => (MouseState::BUTTON_5_DOWN, MouseState::BUTTON_5_UP),
            _ => return Err("Invalid click keycode"),
        };
        self.click_down = click_down;
        self.click_up = click_up;
        Ok(())
    }

    pub fn move_mouse_over_time(&self, dur: Duration, n_chunks: u32, pos: Coord<i32>) {
        let sleep_dur = dur / n_chunks;
        let chunked_x = pos.x / n_chunks as i32;
        let chunked_y = pos.y / n_chunks as i32;

        for _ in 0..n_chunks {
            self.move_mouse_relative(Coord::new(chunked_x, chunked_y));
            spin_sleep::sleep(sleep_dur);
        }
    }

    fn move_mouse_relative(&self, pos: Coord<i32>) {
        let stroke = Stroke::Mouse {
            state: MouseState::MOVE,
            flags: MouseFlags::MOVE_RELATIVE,
            rolling: 0,
            x: pos.x,
            y: pos.y,
            information: 0,
        };
        self.interception.send(self.mouse_dev, &[stroke]);
    }
}

pub fn key_pressed(key_code: u16) -> bool {
    unsafe { GetAsyncKeyState(key_code as i32) < 0 }
}

pub fn wait_for_release(key_code: u16, timeout: Duration) {
    let start = Instant::now();
    while key_pressed(key_code) && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(1));
    }
}

pub fn get_any_pressed_key() -> Result<Option<u16>, &'static str> {
    let mut buf = [0u8; 256];
    if !unsafe { GetKeyboardState(buf.as_mut_ptr()) }.as_bool() {
        return Err("GetKeyboardState failed");
    }
    match buf
        .iter()
        .enumerate()
        .find(|(_, &key_state)| (key_state >> 7) == 1)
    {
        Some((key_code, _)) => Ok(Some(key_code as _)),
        None => Ok(None),
    }
}

pub fn keycode_to_string(key_code: u16) -> Result<String, &'static str> {
    // MapVirtualKeyW doesn't recognize mouse keycodes
    match key_code.into() {
        VK_LBUTTON => return Ok("Mouse1".to_string()),
        VK_RBUTTON => return Ok("Mouse2".to_string()),
        VK_MBUTTON => return Ok("Mouse3".to_string()),
        VK_XBUTTON1 => return Ok("Mouse4".to_string()),
        VK_XBUTTON2 => return Ok("Mouse5".to_string()),
        _ => (),
    }

    const BUF_SIZE: usize = 32;
    let mut buf = [0u16; BUF_SIZE];
    unsafe {
        let scan_code = MapVirtualKeyW(key_code as u32, MAPVK_VK_TO_VSC_EX);
        if scan_code != 0 {
            let str_size = GetKeyNameTextW(
                (scan_code as i32) << 16,
                PWSTR(buf.as_mut_ptr()),
                BUF_SIZE as i32,
            );
            if str_size > 0 {
                Ok(String::from_utf16_lossy(&buf[..str_size as usize]))
            } else {
                Err("GetKeyNameTextW failed")
            }
        } else {
            Err("No translation from keycode to scancode")
        }
    }
}

pub fn find_mouse_dev() -> Result<i32, &'static str> {
    Ok(Interception::new().ok_or(INTERCEPTION_ERR)?.capture_mouse())
}
