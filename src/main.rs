#![allow(dead_code)]
#![allow(unused_imports)]

mod config;
mod coord;
mod gui;
mod image;
mod img_funcs;
mod input;
mod pixel_bot;

use config::{CfgKey, CfgValue, Config};
use gui::{Gui, Message};
use pixel_bot::PixelBot;
use std::panic;
use std::sync::{mpsc, Arc, RwLock};

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
    let config = Arc::new(RwLock::new(Config::from_file().unwrap()));
    let (sender, receiver) = mpsc::channel();

    let d = scrap::Display::primary().unwrap();
    let (width, height) = (d.width() as u32, d.height() as u32);
    drop(d);

    // Setting crop_w and crop_h bounds relative to screen size
    let crop_w = config.read().unwrap().get(CfgKey::CropW).val;
    let crop_h = config.read().unwrap().get(CfgKey::CropH).val;
    config
        .write()
        .unwrap()
        .set(
            CfgKey::CropW,
            &CfgValue::new(crop_w, Some(0..width / 2)),
            true,
        )
        .expect("Crop W out of bounds in config file");
    config
        .write()
        .unwrap()
        .set(
            CfgKey::CropH,
            &CfgValue::new(crop_h, Some(0..height / 2)),
            true,
        )
        .expect("Crop H out of bounds in config file");

    let mut pixel_bot = PixelBot::new(config.clone());
    pixel_bot.start().unwrap(); // blocks waiting for mouse to move

    let mut gui = Gui::new(1000, 1000, config.clone(), sender);
    gui.create_crop_widget(100, 100, width as i32, height as i32, 0.2);
    gui.init();

    while gui.wait() {
        // std::thread::sleep(std::time::Duration::from_millis(1));
        if let Ok(msg) = receiver.try_recv() {
            match msg {
                Message::ChangedConfig => {
                    pixel_bot.reload().unwrap();
                }
            }
        }
    }
}
