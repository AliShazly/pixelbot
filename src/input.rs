use crate::coord::Coord;
use interception::*;
use std::thread;
use std::time;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const CLICK_UP: MouseState = MouseState::MIDDLE_BUTTON_UP;
const CLICK_DOWN: MouseState = MouseState::MIDDLE_BUTTON_DOWN;

trait Empty {
    fn empty() -> Self;
}

impl Empty for Stroke {
    fn empty() -> Self {
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
        println!("Looking for mouse...");
        self.set_filter(is_mouse, Filter::MouseFilter(MouseState::MOVE));
        let mouse_dev = self.wait();
        self.set_filter(is_mouse, Filter::MouseFilter(MouseState::empty()));
        println!("Found mouse");
        mouse_dev
    }
}

pub struct InterceptionState {
    interception: Interception,
    mouse_dev: Device,
}

impl InterceptionState {
    pub fn new(dev: Option<Device>) -> Self {
        let mut interception = Interception::new().expect("Error initializing interception");
        let mouse_dev = match dev {
            Some(d) => d,
            None => interception.capture_mouse(),
        };
        InterceptionState {
            interception,
            mouse_dev,
        }
    }

    pub fn click_down(&self) {
        let mut stroke = Stroke::empty();
        if let Stroke::Mouse { ref mut state, .. } = stroke {
            *state = CLICK_DOWN;
        }
        self.interception.send(self.mouse_dev, &[stroke]);
    }

    pub fn click_up(&self) {
        let mut stroke = Stroke::empty();
        if let Stroke::Mouse { ref mut state, .. } = stroke {
            *state = CLICK_UP;
        }
        self.interception.send(self.mouse_dev, &[stroke]);
    }

    pub fn move_mouse_over_time(&self, dur: time::Duration, n_chunks: u32, pos: Coord<i32>) {
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

pub fn key_pressed(key_code: i32) -> bool {
    unsafe { GetAsyncKeyState(key_code) < 0 }
}

pub fn wait_for_release(key_code: i32, timeout: time::Duration) {
    let now = time::Instant::now();
    while key_pressed(key_code) && now.elapsed() <= timeout {
        thread::sleep(time::Duration::from_millis(1));
    }
}

pub fn find_mouse_dev() -> i32 {
    Interception::new()
        .expect("Error initializing interception")
        .capture_mouse()
}
