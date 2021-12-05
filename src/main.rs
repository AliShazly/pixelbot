mod coord;
mod image;
mod img_funcs;
mod input;

use coord::Coord;
use image::{Image, Pixel, PixelOrder};
use img_funcs::{color_range_avg_pos, crop_to_center};
use input::{move_mouse_relative, InterceptionState};

use scrap;
use std::io::ErrorKind::WouldBlock;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const TARGET_COLOR: Pixel = Pixel {
    //cerise
    r: 196,
    g: 58,
    b: 172,
};
const CROP_W: usize = 1152;
const CROP_H: usize = 592;
const SENS: f32 = 3.33;
const FPS: u32 = 144;
const Y_DIVISOR: f32 = 1.3;
// const AIM_KEYCODE: i32 = 0x06;
const AIM_KEYCODE: i32 = 0x01;
const MAX_PX_MOVE: u32 = 15;

fn main() {
    let one_frame = Duration::new(1, 0) / FPS;

    let display = scrap::Display::primary().unwrap();
    let mut capturer = scrap::Capturer::new(display).unwrap();
    let (screen_w, screen_h) = (capturer.width(), capturer.height());
    println!(
        "Using primary display.\nScreen size: {}x{}",
        screen_w, screen_h
    );

    let mut interception = InterceptionState::new();
    interception.capture_mouse();

    loop {
        // let now = Instant::now();

        // Grab DXGI buffer
        let buffer = match capturer.frame() {
            Ok(buffer) => buffer,
            Err(error) => {
                if error.kind() == WouldBlock {
                    thread::sleep(one_frame);
                    continue;
                } else {
                    panic!("Error: {}", error);
                }
            }
        };

        // Crop image
        let cropped = crop_to_center(
            &Image::new(&(*buffer), PixelOrder::BGRA, screen_w, screen_h),
            CROP_W as usize,
            CROP_H as usize,
        );

        // Search through image and find avg position of the target color
        let relative_coord = match color_range_avg_pos(&cropped, TARGET_COLOR, 0.83, Y_DIVISOR) {
            Some(coord) => Coord::new(
                // making coord relative to center
                coord.x as i32 - (cropped.w / 2) as i32,
                coord.y as i32 - (cropped.h / 2) as i32,
            ),
            None => Coord::new(0, 0),
        };

        // Move mouse to aim coord
        if unsafe { GetAsyncKeyState(AIM_KEYCODE) } < 0 {
            move_mouse_relative(
                &interception,
                ((relative_coord.x as f32 / SENS) as i32).min(MAX_PX_MOVE as i32),
                ((relative_coord.y as f32 / SENS) as i32).min(MAX_PX_MOVE as i32),
            );
        }

        // println!("{:?}", now.elapsed());
    }
}
