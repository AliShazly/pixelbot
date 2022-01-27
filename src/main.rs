#![allow(dead_code)]
#![feature(once_cell)]

mod capture;
mod config;
mod coord;
mod gui;
mod image;
mod input;
mod logging;
mod pixel_bot;

mod svg_drawing;

use config::{Bounded, CfgKey, Config, ParseError, ValType};
use crossbeam::channel;
use gui::Gui;
use logging::log_err;
use pixel_bot::PixelBot;
use std::io::{self, ErrorKind};
use std::panic;
use std::sync::{Arc, RwLock};

const CFG_PATH: &str = "config.cfg";

// Kills the entire process if one thread panics, shows panicinfo in messagebox
fn set_panic_hook() {
    use windows::Win32::{
        Foundation::PWSTR,
        UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR},
    };
    let orig_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        orig_hook(panic_info);

        let mut caption_buf = "Fatal Error\0".encode_utf16().collect::<Vec<_>>();
        let mut text_buf = panic_info
            .payload()
            .downcast_ref()
            .unwrap_or(&"Fatal Error")
            .encode_utf16()
            .collect::<Vec<_>>();
        text_buf.push(0); // null termination
        let lptext = PWSTR(&mut text_buf[0] as *mut _);
        let lpcaption = PWSTR(&mut caption_buf[0] as *mut _);
        unsafe { MessageBoxW(None, lptext, lpcaption, MB_ICONERROR) };

        std::process::exit(1);
    }));
}

fn primary_display_dims() -> (u32, u32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN) as u32,
            GetSystemMetrics(SM_CYSCREEN) as u32,
        )
    }
}

fn main() {
    set_panic_hook();

    let config = Arc::new(RwLock::new(match Config::from_file(CFG_PATH) {
        Ok(cfg) => cfg,
        Err(err) => {
            let default_cfg = Config::default();
            log_err!("Error reading config file:");

            if let Some(e) = err.downcast_ref::<io::Error>() {
                if e.kind() == ErrorKind::NotFound {
                    log_err!("\tConfig file not found, saving default to {}", CFG_PATH);
                    default_cfg.write_to_file(CFG_PATH).unwrap();
                }
                default_cfg
            } else if let Ok(e) = err.downcast::<ParseError>() {
                log_err!("\t{}", e);
                match *e {
                    ParseError::NotExhaustive(ne_cfg, _) => ne_cfg,
                    _ => {
                        log_err!("Falling back to default config");
                        default_cfg
                    }
                }
            } else {
                default_cfg
            }
        }
    }));

    // Setting crop_w and crop_h bounds relative to screen size
    let (screen_w, screen_h) = primary_display_dims();
    let crop_w = ValType::Unsigned(Bounded::new(0, 0..=(screen_w / 2) - 1));
    let crop_h = ValType::Unsigned(Bounded::new(0, 0..=(screen_h / 2) - 1));
    let mut cfg = config.write().unwrap();
    cfg.set_bounds(CfgKey::CropW, crop_w).unwrap();
    cfg.set_bounds(CfgKey::CropH, crop_h).unwrap();
    cfg.is_dirty = false;
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
