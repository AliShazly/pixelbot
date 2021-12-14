use crate::config::{CfgKey, CfgValue, Config};
use fltk::{app::*, enums::*, frame::*, group::*, prelude::*, valuator::*, widget::*, window::*};
use std::assert;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{mpsc, Arc, RwLock};

pub enum Message {
    ChangedConfig,
}

#[derive(Debug)]
struct CropBox {
    bx: Group,
    x_offset: i32,
    y_offset: i32,
    max_w: i32,
    max_h: i32,
}

impl CropBox {
    fn change_bounds(&mut self, x_percent: f64, y_percent: f64) {
        if x_percent > 0. {
            let x_pixels = (x_percent * self.max_w as f64).round() as i32;
            let box_y = self.bx.y();
            let box_h = self.bx.height();
            self.bx.resize(
                (x_pixels / 2) + self.x_offset,
                box_y,
                self.max_w - x_pixels,
                box_h,
            );
        }
        if y_percent > 0. {
            let y_pixels = (y_percent * self.max_h as f64).round() as i32;
            let box_x = self.bx.x();
            let box_w = self.bx.width();
            self.bx.resize(
                box_x,
                (y_pixels / 2) + self.y_offset,
                box_w,
                self.max_h - y_pixels,
            );
        }
    }
}

trait NormalizedVal {
    fn norm_val(&self) -> f64;
}
impl NormalizedVal for HorFillSlider {
    fn norm_val(&self) -> f64 {
        self.value() / self.maximum()
    }
}

pub struct Gui {
    w: i32,
    h: i32,
    app: App,
    window: Rc<RefCell<Window>>,
    crop_box: Rc<RefCell<CropBox>>,
    config: Arc<RwLock<Config>>,
    sender: mpsc::Sender<Message>,
    init_fns: Vec<Box<dyn Fn()>>,
}

impl Gui {
    pub fn new(w: i32, h: i32, config: Arc<RwLock<Config>>, sender: mpsc::Sender<Message>) -> Self {
        let app = App::default();
        let window = Rc::new(RefCell::new(Window::new(w / 2, h / 2, w, h, "ayooo")));

        Self {
            w,
            h,
            window,
            app,
            crop_box: Rc::new(RefCell::new(CropBox {
                bx: Group::default_fill(),
                max_w: 0,
                max_h: 0,
                x_offset: 0,
                y_offset: 0,
            })),
            config,
            sender,
            init_fns: Vec::new(),
        }
    }

    fn cfg_slider(
        &mut self,
        cfg_key: CfgKey,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        label: &'static str,
    ) -> HorFillSlider {
        let mut slider = HorFillSlider::new(x, y, w, h, "");
        let mut label_frame = Frame::new(x, y, w, h, "");
        label_frame.set_label(label);
        label_frame.set_label_font(Font::HelveticaBold);
        label_frame.set_label_size((h as f32 / 2.7) as i32);
        slider.set_color(Color::Dark2);
        slider.set_slider_frame(FrameType::EmbossedBox);

        let cfg_value = self.config.read().unwrap().get(cfg_key);
        let val_bounds = cfg_value.bounds.unwrap();
        slider.set_value(cfg_value.val);
        slider.set_bounds(val_bounds.start, val_bounds.end - 1.); // -1 since bounds are inclusive
        slider.set_precision(2);

        // To make sure the slider label is always drawn on top of the slider
        slider.draw(move |s| {
            s.redraw();
            label_frame.redraw_label();
        });

        let config = self.config.clone();
        let sender = self.sender.clone();
        slider.handle(move |slider, ev| match ev {
            Event::Released => {
                config
                    .write()
                    .unwrap()
                    .set(cfg_key, &CfgValue::new(slider.value(), None), false)
                    .unwrap();
                sender.send(Message::ChangedConfig).unwrap();
                true
            }
            _ => false,
        });
        slider
    }

    pub fn create_crop_widget(
        &mut self,
        x: i32,
        y: i32,
        screen_w: i32,
        screen_h: i32,
        scalar: f32,
    ) {
        assert!(std::ops::RangeInclusive::new(0., 1.).contains(&scalar));

        let box_w = (screen_w as f32 * scalar) as i32;
        let box_h = (screen_h as f32 * scalar) as i32;
        let slider_h = self.h / 20;
        let slider1_ypos = y + box_h;
        let slider2_ypos = slider1_ypos + slider_h;

        let mut bg_box = Group::new(x, y, box_w, box_h, "");
        bg_box.set_frame(FrameType::FlatBox);
        bg_box.set_color(Color::Red);
        bg_box.end();

        let mut fg_box = Group::new(x, y, box_w, box_h, "");
        fg_box.set_frame(FrameType::FlatBox);
        fg_box.set_color(Color::Green);
        fg_box.end();

        self.crop_box = Rc::new(RefCell::new(CropBox {
            bx: fg_box,
            max_w: box_w,
            max_h: box_h,
            x_offset: x,
            y_offset: y,
        }));

        let mut slider1 =
            self.cfg_slider(CfgKey::CropW, x, slider1_ypos, box_w, slider_h, "Crop X");
        let mut slider2 =
            self.cfg_slider(CfgKey::CropH, x, slider2_ypos, box_w, slider_h, "Crop Y");

        let crop_box = self.crop_box.clone();
        let window = self.window.clone();
        slider1.set_callback(move |slider| {
            crop_box
                .try_borrow_mut()
                .unwrap()
                .change_bounds(slider.norm_val(), 0.);
            window.try_borrow_mut().unwrap().redraw();
        });

        let crop_box = self.crop_box.clone();
        let window = self.window.clone();
        slider2.set_callback(move |slider| {
            crop_box
                .try_borrow_mut()
                .unwrap()
                .change_bounds(0., slider.norm_val());
            window.try_borrow_mut().unwrap().redraw();
        });

        let crop_box = self.crop_box.clone();
        let (init_x_percent, init_y_percent) = (slider1.norm_val(), slider2.norm_val());
        self.init_fns.push(Box::new(move || {
            crop_box
                .try_borrow_mut()
                .unwrap()
                .change_bounds(init_x_percent, init_y_percent);
        }));
    }

    pub fn init(&mut self) {
        self.window.borrow().end();
        self.window.try_borrow_mut().unwrap().show();
        for init_fn in self.init_fns.iter() {
            init_fn();
        }
    }

    pub fn wait(&self) -> bool {
        self.app.wait()
    }
}
