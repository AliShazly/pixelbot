#![allow(dead_code)]

mod config;
mod coord;
mod gui;
mod image;
mod input;
mod pixel_bot;

use config::{CfgKey, CfgValue, Config};
use gui::{Bounds, Gui, Message};
use image::Color;
use pixel_bot::PixelBot;
use std::panic;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;

use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

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
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    let config = Arc::new(RwLock::new(Config::from_file().unwrap()));

    let d = scrap::Display::primary().unwrap();
    let (screen_width, screen_height) = (d.width() as u32, d.height() as u32);
    drop(d);

    // Setting crop_w and crop_h bounds relative to screen size
    let crop_w = config.read().unwrap().get(CfgKey::CropW).val;
    let crop_h = config.read().unwrap().get(CfgKey::CropH).val;
    config
        .write()
        .unwrap()
        .set(
            CfgKey::CropW,
            &CfgValue::new(crop_w, Some(0..screen_width / 2)),
            true,
        )
        .expect("Crop W out of bounds in config file");
    config
        .write()
        .unwrap()
        .set(
            CfgKey::CropH,
            &CfgValue::new(crop_h, Some(0..screen_height / 2)),
            true,
        )
        .expect("Crop H out of bounds in config file");

    // gui_sender passed to gui, receiver stays in main thread
    let (gui_sender, gui_receiver) = mpsc::channel();
    // graph_sender passed to pixelbot, receiver passed to gui callback
    let (graph_sender, graph_receiver) = mpsc::channel();

    let mut pixel_bot = PixelBot::new(config.clone());
    pixel_bot.start(graph_sender).unwrap(); // blocks waiting for mouse to move

    let mut gui = Gui::new(1000, 1000, config.clone(), gui_sender);
    gui.create_crop_widget(
        Bounds::new(100, 100, screen_width as i32, screen_height as i32),
        0.1,
    );
    gui.create_graph(
        Bounds::new(300, 300, 600, 300),
        graph_receiver,
        5..50,
        Color::new(255, 0, 0, 255),
    );
    gui.init();

    while gui.wait() {
        thread::sleep(Duration::from_millis(1)); //TODO: instead of a sleep here, remove the app::add_idle and make an event that trips when graph data is ready
        if let Ok(msg) = gui_receiver.try_recv() {
            match msg {
                Message::ChangedConfig => {
                    pixel_bot.reload().unwrap();
                }
            }
        }
    }
}
