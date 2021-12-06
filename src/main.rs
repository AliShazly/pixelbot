mod coord;
mod image;
mod img_funcs;
mod input;

use coord::Coord;
use image::{Image, Pixel, PixelOrder};
use img_funcs::{color_range_avg_pos, crop_to_center};
use input::{key_pressed, wait_for_release, InterceptionState};

use rand::{self, Rng};
use scrap;
use std::io::ErrorKind::WouldBlock;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const TARGET_COLOR: Pixel = Pixel {
    //cerise
    r: 196,
    g: 58,
    b: 172,
};
const CROP_W: usize = 1152;
const CROP_H: usize = 592;
const COLOR_THRESH: f32 = 0.83;
const SENS: f32 = 3.;
const FPS: u32 = 144;
const Y_DIVISOR: f32 = 1.3;
const AIM_KEYCODE: i32 = 0x01; //0x06
const TOGGLE_KEYCODE: i32 = 0xBE; // .
const TOGGLE_AUTOCLICK_KEYCODE: i32 = 0xBC; // ,
const AUTOCLICK_INBTWN_SLEEP_MS: std::ops::Range<u64> = 19..66;
const AIM_DURATION: Duration = Duration::from_micros(50);
const AIM_STEPS: u32 = 2;

pub fn spawn_click_thread() {
    thread::spawn(|| {
        #[derive(Debug)]
        enum ClickMode {
            RegularClick,          // Good ole bread and butter, the classic
            AutoClick,             // Repeatedly presses mmb when holding lmb
            RedirectedClick(bool), // mmb presses mirror lmb clicks, stores whether pressed
        }

        let mut click_mode = ClickMode::RegularClick;
        let mut interception = InterceptionState::new();
        interception.capture_mouse();

        let mut rng = rand::thread_rng();

        println!("Clickmode: {:?}. Starting click thread...", click_mode);
        loop {
            thread::sleep(Duration::from_millis(1));

            // Cycling to the next clickmode when the toggle key is pressed
            if key_pressed(TOGGLE_AUTOCLICK_KEYCODE) {
                click_mode = match click_mode {
                    ClickMode::RegularClick => ClickMode::AutoClick,
                    ClickMode::AutoClick => ClickMode::RedirectedClick(false),
                    ClickMode::RedirectedClick(is_pressed) => {
                        // if the clickmode was cycled while redirectedclick was pressed down, we reset it.
                        if is_pressed {
                            interception.click_up()
                        }
                        ClickMode::RegularClick
                    }
                };
                println!("Toggled clickmode to {:?}.", click_mode);
                wait_for_release(TOGGLE_AUTOCLICK_KEYCODE);
            }

            // Handle each clickmode scenario
            match click_mode {
                ClickMode::RegularClick => {}

                ClickMode::AutoClick => {
                    if key_pressed(AIM_KEYCODE) {
                        interception.click_down();
                        thread::sleep(Duration::from_millis(
                            rng.gen_range(AUTOCLICK_INBTWN_SLEEP_MS),
                        ));
                        interception.click_up();
                        thread::sleep(Duration::from_millis(
                            rng.gen_range(AUTOCLICK_INBTWN_SLEEP_MS),
                        ));
                    }
                }
                ClickMode::RedirectedClick(ref mut is_pressed) => {
                    if key_pressed(AIM_KEYCODE) {
                        if !*is_pressed {
                            interception.click_down();
                            *is_pressed = true;
                        }
                    } else {
                        if *is_pressed {
                            interception.click_up();
                            *is_pressed = false;
                        }
                    }
                }
            }
        }
    });
}

pub fn spawn_aim_thread(receiver: mpsc::Receiver<Coord<i32>>) {
    thread::spawn(move || {
        let mut enabled = true;
        let mut interception = InterceptionState::new();
        interception.capture_mouse();

        println!(
            "Aim {}. Starting aim thread...",
            if enabled { "enabled" } else { "disabled" }
        );
        loop {
            // getting the most recent item in the reciever queue
            match receiver.try_iter().last() {
                Some(coord) => {
                    if enabled && key_pressed(AIM_KEYCODE) {
                        interception.move_mouse_over_time(AIM_DURATION, AIM_STEPS, coord);
                    }
                    if key_pressed(TOGGLE_KEYCODE) {
                        enabled = !enabled;
                        println!("Aim {}.", if enabled { "enabled" } else { "disabled" });
                        wait_for_release(TOGGLE_KEYCODE);
                    }
                }
                None => {}
            }
        }
    });
}

fn main() {
    let one_frame = Duration::new(1, 0) / FPS;

    let display = scrap::Display::primary().unwrap();
    let mut capturer = scrap::Capturer::new(display).unwrap();
    let (screen_w, screen_h) = (capturer.width(), capturer.height());
    println!(
        "Using primary display.\nScreen size: {}x{}",
        screen_w, screen_h
    );

    let (sender, receiver) = mpsc::channel();
    spawn_aim_thread(receiver);
    spawn_click_thread();

    println!("Init finished");
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
        let relative_coord =
            match color_range_avg_pos(&cropped, TARGET_COLOR, COLOR_THRESH, Y_DIVISOR) {
                Some(coord) => Coord::new(
                    // making coord relative to center
                    coord.x as i32 - (cropped.w / 2) as i32,
                    coord.y as i32 - (cropped.h / 2) as i32,
                ),
                None => Coord::new(0, 0),
            };

        let aim_x = (relative_coord.x as f32 / SENS) as i32;
        let aim_y = (relative_coord.y as f32 / SENS) as i32;

        sender.send(Coord::new(aim_x, aim_y)).unwrap();

        // println!("{:?}", now.elapsed());
    }
}
