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

pub struct InterceptionState {
    interception: Interception,
    mouse_dev: Device,
}

impl InterceptionState {
    pub fn new() -> Self {
        InterceptionState {
            interception: Interception::new().expect("Error initializing interception"),
            mouse_dev: -1,
        }
    }
    pub fn capture_mouse(&mut self) {
        println!("Looking for mouse...");
        self.interception
            .set_filter(is_mouse, Filter::MouseFilter(MouseState::MOVE));
        self.mouse_dev = self.interception.wait();
        self.interception
            .set_filter(is_mouse, Filter::MouseFilter(MouseState::empty()));
        println!("Found mouse");
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
            thread::sleep(sleep_dur);
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

pub fn wait_for_release(key_code: i32) {
    while key_pressed(key_code) {
        // wait for release
    }
}
