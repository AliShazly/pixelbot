use crate::config::{Bounded, CfgKey, Config, ValType};
use crate::coord::Coord;
use crate::image::{Bgra8, Color, Image};
use crate::input::{find_mouse_dev, key_pressed, wait_for_release, InterceptionState};
use crate::logging::{log, log_err};

use crossbeam::channel::{self, Receiver, Sender};
use rand::{self, Rng};
use std::io::ErrorKind::WouldBlock;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::Instant;

pub struct CapData {
    pub img: Image<Vec<u8>, Bgra8>,
    pub target_coords: Option<Vec<Coord<usize>>>,
    pub aim_coord: Option<Coord<usize>>,
}
pub enum Message {
    IterTime(Duration),
    CaptureData(CapData),
}

enum ThreadMsg {
    Stop,
    Reload,
}

pub struct PixelBot {
    config: Arc<RwLock<Config>>,
    handles: Vec<JoinHandle<()>>,
    aim_thread_sender: Option<Sender<ThreadMsg>>,
    click_thread_sender: Option<Sender<ThreadMsg>>,
    mouse_dev: Option<i32>,
}

impl Drop for PixelBot {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl PixelBot {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        Self {
            config,
            handles: Vec::new(),
            aim_thread_sender: None,
            click_thread_sender: None,
            mouse_dev: None,
        }
    }

    pub fn start(&mut self, gui_sender: Sender<Message>) -> Result<(), &'static str> {
        if !self.handles.is_empty() {
            return Err("Already started");
        }

        let (aim_sender, aim_receiver) = channel::unbounded();
        let (click_sender, click_receiver) = channel::unbounded();
        self.aim_thread_sender = Some(aim_sender);
        self.click_thread_sender = Some(click_sender);
        self.mouse_dev = Some(find_mouse_dev()?);

        self.handles
            .push(self.spawn_aim_thread(gui_sender, aim_receiver));
        self.handles.push(self.spawn_click_thread(click_receiver));
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), &'static str> {
        if self.handles.is_empty() {
            return Err("Already stopped");
        }

        std::mem::take(&mut self.aim_thread_sender)
            .unwrap()
            .send(ThreadMsg::Stop)
            .unwrap();
        std::mem::take(&mut self.click_thread_sender)
            .unwrap()
            .send(ThreadMsg::Stop)
            .unwrap();

        while let Some(handle) = self.handles.pop() {
            handle.join().unwrap();
        }
        Ok(())
    }

    pub fn reload(&mut self) -> Result<(), &'static str> {
        if self.handles.is_empty() {
            return Err("Not Started");
        }

        self.aim_thread_sender
            .as_ref()
            .unwrap()
            .send(ThreadMsg::Reload)
            .unwrap();
        self.click_thread_sender
            .as_ref()
            .unwrap()
            .send(ThreadMsg::Reload)
            .unwrap();

        Ok(())
    }

    fn spawn_aim_thread(
        &self,
        gui_sender: Sender<Message>,
        thread_rx: Receiver<ThreadMsg>,
    ) -> JoinHandle<()> {
        let config = self.config.clone();
        let mouse_dev = self.mouse_dev.unwrap();

        thread::spawn(move || {
            let mut enabled = true;
            let mut capturer = scrap::Capturer::new(scrap::Display::primary().unwrap()).unwrap();
            let (screen_w, screen_h) = (capturer.width(), capturer.height());
            let interception = InterceptionState::new(mouse_dev).unwrap();
            log!(
                "Starting aim thread on primary display\nScreen size: {}x{}",
                screen_w,
                screen_h
            );

            let mut last_iter = Instant::now();
            'outer: loop {
                let cfg = config.read().unwrap();
                let fps: u32 = <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::Fps)).val;
                let crop_w: u32 = <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::CropW)).val;
                let crop_h: u32 = <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::CropH)).val;
                let color_thresh: f32 =
                    <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::ColorThresh)).val;
                let aim_divisor: f32 =
                    <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::AimDivisor)).val;
                // let y_divisor: f32 =
                //     <ValType as Into<Bounded<_>>>::into(*cfg.get(CfgKey::YDivisor)).val;
                let aim_dur: u32 =
                    <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::AimDurationMicros)).val;
                let aim_steps: u32 =
                    <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::AimSteps)).val;
                let aim_key = <ValType as Into<i32>>::into(cfg.get(CfgKey::AimKeycode));
                let toggle_key = <ValType as Into<i32>>::into(cfg.get(CfgKey::ToggleAimKeycode));
                let target_color = <ValType as Into<Color<u8>>>::into(cfg.get(CfgKey::TargetColor));
                drop(cfg);

                loop {
                    if let Ok(msg) = thread_rx.try_recv() {
                        match msg {
                            ThreadMsg::Reload => break,
                            ThreadMsg::Stop => break 'outer,
                        }
                    }

                    if key_pressed(toggle_key) {
                        enabled = !enabled;
                        log!("Aim {}.", if enabled { "enabled" } else { "disabled" });
                        wait_for_release(toggle_key, Duration::from_millis(500));
                    }

                    if !enabled {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }

                    // Grab DXGI buffer
                    let buffer = match capturer.frame() {
                        Ok(buffer) => buffer,
                        Err(error) => {
                            if error.kind() == WouldBlock {
                                spin_sleep::sleep(Duration::from_secs_f32(1. / fps as f32));
                                continue;
                            } else {
                                panic!("Error: {}", error);
                            }
                        }
                    };

                    // Crop image
                    let buf_img: Image<_, Bgra8> = Image::new(&(*buffer), screen_w, screen_h);
                    let cropped = buf_img.crop_to_center(crop_w as usize, crop_h as usize);

                    // to send to gui for visualizations
                    let mut target_coords: Option<Vec<Coord<usize>>> = None; // Vec of detected pixel coords
                    let mut aim_coord: Option<Coord<usize>> = None; // Average of all the detected pixel coords

                    // Search through image and find avg position of the target color
                    let mut relative_coord = match cropped.detect_color(target_color, color_thresh)
                    {
                        Some(coords) => {
                            let count = coords.len();

                            // Getting avg position of detected points
                            let mut coord_sum = Coord::new(0, 0);
                            coords.iter().for_each(|&coord| coord_sum += coord);
                            let coords_avg = Coord::new(
                                coord_sum.x / count,
                                coord_sum.y / count, // TODO: Need to make make this lower to bias aim towards head, y_divisor is broke af
                            );

                            target_coords = Some(coords);
                            aim_coord = Some(coords_avg);

                            // making coord relative to center
                            Coord::new(
                                coords_avg.x as i32 - (cropped.w / 2) as i32,
                                coords_avg.y as i32 - (cropped.h / 2) as i32,
                            )
                        }
                        None => Coord::new(0, 0),
                    };

                    // scaling for sensitivity
                    relative_coord.x = (relative_coord.x as f32 / aim_divisor) as i32;
                    relative_coord.y = (relative_coord.y as f32 / aim_divisor) as i32;

                    if key_pressed(aim_key) {
                        interception.move_mouse_over_time(
                            Duration::from_micros(aim_dur as u64),
                            aim_steps,
                            relative_coord,
                        );
                    }

                    let _ = gui_sender.try_send(Message::CaptureData(CapData {
                        img: cropped,
                        target_coords,
                        aim_coord,
                    }));
                    let _ = gui_sender.try_send(Message::IterTime(last_iter.elapsed()));
                    last_iter = Instant::now();
                }
            }
        })
    }

    fn spawn_click_thread(&self, thread_rx: Receiver<ThreadMsg>) -> JoinHandle<()> {
        let config = self.config.clone();
        let mouse_dev = self.mouse_dev.unwrap();

        thread::spawn(move || {
            #[derive(Debug)]
            enum ClickMode {
                Regular,          // Good ole bread and butter, the classic
                Auto,             // Repeatedly clicks mmb when holding autoclick key
                Redirected(bool), // mmb clicks mirror autoclick key clicks, stores whether pressed
            }
            let mut click_mode = ClickMode::Regular;
            let mut interception = InterceptionState::new(mouse_dev).unwrap();
            let mut rng = rand::thread_rng();
            log!("Clickmode: {:?}\nStarting click thread", click_mode);

            'outer: loop {
                let cfg = config.read().unwrap();
                let autoclick_key: i32 =
                    <ValType as Into<i32>>::into(cfg.get(CfgKey::AutoclickKeycode));
                let toggle_autoclick_key: i32 =
                    <ValType as Into<i32>>::into(cfg.get(CfgKey::ToggleAutoclickKeycode));
                let fake_lmb_key: i32 =
                    <ValType as Into<i32>>::into(cfg.get(CfgKey::FakeLmbKeycode));
                let mut max_sleep: u32 =
                    <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::MaxAutoclickSleepMs)).val;
                let mut min_sleep: u32 =
                    <ValType as Into<Bounded<_>>>::into(cfg.get(CfgKey::MinAutoclickSleepMs)).val;
                drop(cfg);

                if interception.set_click_keycode(fake_lmb_key).is_err() {
                    log_err!(
                        "Invalid value for {}, using default",
                        CfgKey::FakeLmbKeycode.as_string()
                    );
                }

                if max_sleep < min_sleep {
                    log_err!(
                        "{} shouldn't be less than {}\n Using swapped values",
                        CfgKey::MaxAutoclickSleepMs.as_string(),
                        CfgKey::MinAutoclickSleepMs.as_string()
                    );
                    std::mem::swap(&mut max_sleep, &mut min_sleep);
                }

                loop {
                    thread::sleep(Duration::from_millis(1));

                    if let Ok(msg) = thread_rx.try_recv() {
                        match msg {
                            ThreadMsg::Reload => break,
                            ThreadMsg::Stop => break 'outer,
                        }
                    }

                    // Cycling to the next clickmode when the toggle key is pressed
                    if key_pressed(toggle_autoclick_key) {
                        click_mode = match click_mode {
                            ClickMode::Regular => ClickMode::Auto,
                            ClickMode::Auto => ClickMode::Redirected(false),
                            ClickMode::Redirected(is_pressed) => {
                                // if the clickmode was cycled while redirectedclick was pressed down, we reset it.
                                if is_pressed {
                                    interception.click_up()
                                }
                                ClickMode::Regular
                            }
                        };
                        log!("Toggled clickmode to {:?}.", click_mode);
                        wait_for_release(toggle_autoclick_key, Duration::from_millis(500));
                    }

                    match click_mode {
                        ClickMode::Regular => {}
                        ClickMode::Auto => {
                            if key_pressed(autoclick_key) {
                                interception.click_down();
                                spin_sleep::sleep(Duration::from_millis(
                                    rng.gen_range(min_sleep..max_sleep).into(),
                                ));
                                interception.click_up();
                                spin_sleep::sleep(Duration::from_millis(
                                    rng.gen_range(min_sleep..max_sleep).into(),
                                ));
                            }
                        }
                        ClickMode::Redirected(ref mut was_pressed) => {
                            if key_pressed(autoclick_key) {
                                if !*was_pressed {
                                    interception.click_down();
                                    *was_pressed = true;
                                }
                            } else if *was_pressed {
                                interception.click_up();
                                *was_pressed = false;
                            }
                        }
                    }
                }
            }
        })
    }
}
