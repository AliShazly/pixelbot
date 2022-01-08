#![allow(dead_code)]
#![feature(once_cell)]

mod config;
mod coord;
mod gui;
mod image;
mod input;
mod logging;
mod pixel_bot;

use config::{Bounded, CfgKey, Config, ParseError, ValType};
use crossbeam::channel;
use gui::Gui;
use logging::log_err;
use pixel_bot::PixelBot;
use std::io::{self, ErrorKind};
use std::panic;
use std::sync::{Arc, RwLock};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_SYSTEM_DPI_AWARE};

const CFG_PATH: &str = "config.cfg";

// Kills the entire process if one thread panics
fn set_panic_hook() {
    let orig_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        orig_hook(panic_info);
        std::process::exit(1);
    }));
}

fn main() {
    set_panic_hook();

    // windows scaling messes with fltk's svgbased frames
    unsafe {
        let _ = SetProcessDpiAwareness(PROCESS_SYSTEM_DPI_AWARE);
    }

    let config = Arc::new(RwLock::new(match Config::from_file(CFG_PATH) {
        Ok(cfg) => cfg,
        Err(err) => {
            let default_cfg = Config::default();
            log_err!("Error reading config file:");
            if err.is::<ParseError>() {
                log_err!("\t{}", err);
                match *err.downcast::<ParseError>().unwrap() {
                    ParseError::NotExhaustive(ne_cfg, _) => ne_cfg,
                    _ => {
                        log_err!("Falling back to default config");
                        default_cfg
                    }
                }
            } else if err.is::<io::Error>() {
                if err.downcast::<io::Error>().unwrap().kind() == ErrorKind::NotFound {
                    log_err!("\tConfig file not found, saving default to {}", CFG_PATH);
                    default_cfg.write_to_file(CFG_PATH).unwrap();
                }
                default_cfg
            } else {
                default_cfg
            }
        }
    }));

    // Setting crop_w and crop_h bounds relative to screen size
    let display = scrap::Display::primary().unwrap();
    let (screen_w, screen_h) = (display.width() as u32, display.height() as u32);
    let crop_w = ValType::Unsigned(Bounded::new(0, 0..=(screen_w / 2) - 1));
    let crop_h = ValType::Unsigned(Bounded::new(0, 0..=(screen_h / 2) - 1));
    let mut cfg = config.write().unwrap();
    cfg.set_bounds(CfgKey::CropW, crop_w).unwrap();
    cfg.set_bounds(CfgKey::CropH, crop_h).unwrap();
    drop(display);
    drop(cfg);

    let (gui_sender, gui_receiver) = channel::unbounded();
    let pixel_bot = std::sync::Mutex::new(PixelBot::new(config.clone()));

    crossbeam::scope(|s| {
        // calling start in a thread to avoid blocking while looking for mouse
        s.spawn(|_| {
            if let Err(msg) = pixel_bot.lock().unwrap().start(gui_sender) {
                log_err!("{}", msg); // Interception driver not installed error
            }
        });

        let mut gui = Gui::new(1000, 1000, config.clone());
        gui.init(screen_h as f32 / screen_w as f32, gui_receiver, CFG_PATH);
        while gui.wait(0.01) {
            if config.read().unwrap().is_dirty {
                pixel_bot.lock().unwrap().reload().unwrap();
                config.write().unwrap().is_dirty = false;
            }
        }
    })
    .unwrap();
}
