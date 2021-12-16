use crate::config::{CfgKey, CfgValue, Config};
use crate::coord::Coord;
use crate::image::{Image, Pixel, PixelOrder};
use crate::input::{find_mouse_dev, key_pressed, wait_for_release, InterceptionState};

use rand::{self, Rng};
use std::io::ErrorKind::WouldBlock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::{self, Instant};

const TARGET_COLOR: Pixel = Pixel {
    //cerise
    r: 196,
    g: 58,
    b: 172,
    a: 255,
};

enum Message {
    Reload,
    Stop,
}

pub struct PixelBot {
    config: Arc<RwLock<Config>>,
    handles: Vec<JoinHandle<()>>,
    aim_thread_sender: Option<mpsc::Sender<Message>>,
    click_thread_sender: Option<mpsc::Sender<Message>>,
    mouse_dev: i32,
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
            mouse_dev: find_mouse_dev(),
        }
    }

    pub fn start(&mut self, time_sender: mpsc::Sender<Duration>) -> Result<(), &'static str> {
        if self.aim_thread_sender.is_some() || self.click_thread_sender.is_some() {
            return Err("Already started");
        }
        let (m_sx, m_rx) = mpsc::channel();
        let (c_sx, c_rx) = mpsc::channel();
        self.aim_thread_sender = Some(m_sx);
        self.click_thread_sender = Some(c_sx);
        self.handles.push(self.spawn_aim_thread(m_rx, time_sender));
        self.handles.push(self.spawn_click_thread(c_rx));
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), &'static str> {
        const ERR_MSG: &str = "Already stopped";
        self.aim_thread_sender
            .as_ref()
            .ok_or(ERR_MSG)?
            .send(Message::Stop)
            .unwrap();
        self.click_thread_sender
            .as_ref()
            .ok_or(ERR_MSG)?
            .send(Message::Stop)
            .unwrap();
        while let Some(handle) = self.handles.pop() {
            handle.join().unwrap();
        }
        Ok(())
    }

    pub fn reload(&self) -> Result<(), &'static str> {
        const ERR_MSG: &str = "Not Started";
        self.aim_thread_sender
            .as_ref()
            .ok_or(ERR_MSG)?
            .send(Message::Reload)
            .unwrap();
        self.click_thread_sender
            .as_ref()
            .ok_or(ERR_MSG)?
            .send(Message::Reload)
            .unwrap();
        Ok(())
    }

    fn spawn_aim_thread(
        &self,
        receiver: mpsc::Receiver<Message>,
        time_sender: mpsc::Sender<Duration>,
    ) -> JoinHandle<()> {
        let config = self.config.clone();
        let mouse_dev = self.mouse_dev;

        thread::spawn(move || {
            let mut enabled = true;
            let mut capturer = scrap::Capturer::new(scrap::Display::primary().unwrap()).unwrap();
            let (screen_w, screen_h) = (capturer.width(), capturer.height());
            let interception = InterceptionState::new(Some(mouse_dev));
            println!(
                "Capturing primary display.\nScreen size: {}x{}",
                screen_w, screen_h
            );
            'outer: loop {
                let cfg = config.read().unwrap();
                let fps = cfg.get::<u32>(CfgKey::Fps).val;
                let crop_w = cfg.get::<u32>(CfgKey::CropW).val;
                let crop_h = cfg.get::<u32>(CfgKey::CropH).val;
                let color_thresh = cfg.get(CfgKey::ColorThresh).val;
                let aim_divisor = cfg.get::<f32>(CfgKey::AimDivisor).val;
                let y_divisor = cfg.get(CfgKey::YDivisor).val;
                let aim_dur = Duration::from_micros(cfg.get(CfgKey::AimDurationMicros).val);
                let aim_steps = cfg.get(CfgKey::AimSteps).val;
                let aim_key = cfg.get(CfgKey::AimKeycode).val;
                let toggle_key = cfg.get(CfgKey::ToggleAimKeycode).val;
                drop(cfg);

                let mut last_iter = Instant::now();
                loop {
                    if let Ok(msg) = receiver.try_recv() {
                        match msg {
                            Message::Reload => break,
                            Message::Stop => break 'outer,
                        }
                    }

                    if key_pressed(toggle_key) {
                        enabled = !enabled;
                        println!("Aim {}.", if enabled { "enabled" } else { "disabled" });
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
                    let buf_img = Image::new(&(*buffer), PixelOrder::BGRA, screen_w, screen_h);
                    let cropped = buf_img.crop_to_center(crop_w as usize, crop_h as usize);

                    // Search through image and find avg position of the target color
                    let mut relative_coord =
                        match cropped.color_range_avg_pos(TARGET_COLOR, color_thresh, y_divisor) {
                            Some(coord) => Coord::new(
                                // making coord relative to center
                                coord.x as i32 - (cropped.w / 2) as i32,
                                coord.y as i32 - (cropped.h / 2) as i32,
                            ),
                            None => Coord::new(0, 0),
                        };

                    // scaling for sensitivity
                    relative_coord.x = (relative_coord.x as f32 / aim_divisor) as i32;
                    relative_coord.y = (relative_coord.y as f32 / aim_divisor) as i32;

                    if key_pressed(aim_key) {
                        interception.move_mouse_over_time(aim_dur, aim_steps, relative_coord);
                    }

                    time_sender.send(last_iter.elapsed()).unwrap();
                    last_iter = Instant::now();
                }
            }
        })
    }

    fn spawn_click_thread(&self, receiver: mpsc::Receiver<Message>) -> JoinHandle<()> {
        let config = self.config.clone();
        let mouse_dev = self.mouse_dev;

        thread::spawn(move || {
            #[derive(Debug)]
            enum ClickMode {
                Regular,          // Good ole bread and butter, the classic
                Auto,             // Repeatedly clicks mmb when holding autoclick key
                Redirected(bool), // mmb clicks mirror autoclick key clicks, stores whether pressed
            }
            let mut click_mode = ClickMode::Regular;
            let interception = InterceptionState::new(Some(mouse_dev));
            let mut rng = rand::thread_rng();
            println!("Clickmode: {:?}. Starting click thread...", click_mode);

            'outer: loop {
                let cfg = config.read().unwrap();
                let autoclick_key = cfg.get(CfgKey::AutoclickKeycode).val;
                let toggle_autoclick_key = cfg.get(CfgKey::ToggleAutoclickKeycode).val;
                let max_sleep = cfg.get::<u32>(CfgKey::MaxAutoclickSleepMs).val;
                let min_sleep = cfg.get::<u32>(CfgKey::MinAutoclickSleepMs).val;
                drop(cfg);

                loop {
                    thread::sleep(Duration::from_millis(1));

                    if let Ok(msg) = receiver.try_recv() {
                        match msg {
                            Message::Reload => break,
                            Message::Stop => break 'outer,
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
                        println!("Toggled clickmode to {:?}.", click_mode);
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
