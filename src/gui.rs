use crate::config::{CfgKey, CfgVal, Config};
use fltk::{app::*, enums::*, frame::*, group::*, prelude::*, valuator::*, widget::*, window::*};
use std::assert;

#[derive(Clone, Copy)]
enum Message {
    ChangedCropBounds(f64, f64),
}

struct CropBox {
    bx: Group,
    max_w: i32,
    max_h: i32,
}

pub struct Gui<'a> {
    w: i32,
    h: i32,
    app: App,
    window: Window,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
    crop_box: CropBox,
    init_messages: Vec<Message>,
    config: &'a mut Config,
}

impl<'a> Gui<'a> {
    pub fn new(w: i32, h: i32, config: &'a mut Config) -> Self {
        let app = App::default();
        let (sender, receiver) = channel::<Message>();
        let window = Window::new(w / 2, h / 2, w, h, "ayooo");

        Self {
            w,
            h,
            window,
            app,
            sender,
            receiver,
            crop_box: CropBox {
                bx: Group::default_fill(),
                max_w: 0,
                max_h: 0,
            },
            init_messages: Vec::new(),
            config,
        }
    }

    // https://stackoverflow.com/questions/3971841/how-to-resize-images-proportionally-keeping-the-aspect-ratio
    fn fit_size_to_aspect_ratio(orig_w: i32, orig_h: i32, cur_w: i32, cur_h: i32) -> (i32, i32) {
        let ratio = (cur_w / orig_w).min(cur_h / orig_h);
        (orig_w * ratio, orig_h * ratio)
    }

    fn create_slider(
        &self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        label: &'static str,
        cfg_key: CfgKey,
    ) -> HorFillSlider {
        // let inner_pack = Pack::new(pack.x, pack.y, pack.w, pack.h, "");
        let mut slider = HorFillSlider::new(x, y, w, h, "");
        let mut label_frame = Frame::new(x, y, w, h, "");
        label_frame.set_label(label);
        label_frame.set_label_font(Font::HelveticaBold);
        label_frame.set_label_size((h as f32 / 2.7) as i32);
        slider.set_color(Color::Dark2);
        slider.set_slider_frame(FrameType::EmbossedBox);

        let cfg_val = self.config.get(cfg_key);
        let (val, start, end) = match cfg_val {
            CfgVal::U32(val, bounds) => (*val as f64, *bounds.start() as f64, *bounds.end() as f64),
            CfgVal::F32(val, bounds) => (*val as f64, *bounds.start() as f64, *bounds.end() as f64),
            _ => panic!("Invalid value for slider bounds"),
        };
        slider.set_value(val);
        slider.set_bounds(start, end);
        slider.set_precision(2);

        slider.draw(move |s| {
            s.redraw();
            label_frame.redraw_label();
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

        self.crop_box = CropBox {
            bx: fg_box,
            max_w: box_w,
            max_h: box_h,
        };

        let mut slider1 =
            self.create_slider(x, slider1_ypos, box_w, slider_h, "Crop X", CfgKey::CropW);
        let mut slider2 =
            self.create_slider(x, slider2_ypos, box_w, slider_h, "Crop Y", CfgKey::CropH);

        let x_sender = self.sender;
        slider1.handle(move |widg, ev| match ev {
            Event::Drag => {
                x_sender.send(Message::ChangedCropBounds(
                    widg.value() as f64 / widg.maximum(),
                    0.,
                ));
                true
            }
            _ => false,
        });
        let y_sender = self.sender;
        slider2.handle(move |widg, ev| match ev {
            Event::Drag => {
                y_sender.send(Message::ChangedCropBounds(
                    0.,
                    widg.value() as f64 / widg.maximum(),
                ));
                true
            }
            _ => false,
        });

        self.init_messages.push(Message::ChangedCropBounds(
            slider1.value() as f64 / slider1.maximum(),
            slider2.value() as f64 / slider2.maximum(),
        ));
    }

    pub fn main_loop(&mut self) {
        self.window.end();
        self.window.show();

        for message in self.init_messages.iter() {
            self.sender.send(*message);
        }

        while self.app.wait() {
            if let Some(msg) = self.receiver.recv() {
                match msg {
                    Message::ChangedCropBounds(x_percent, y_percent) => {
                        let max_w = self.crop_box.max_w;
                        let max_h = self.crop_box.max_h;
                        if x_percent > 0. {
                            let x_pixels = (x_percent * max_w as f64).round() as i32;
                            self.crop_box.bx.resize(
                                x_pixels / 2,
                                self.crop_box.bx.y(),
                                max_w - x_pixels,
                                self.crop_box.bx.height(),
                            );
                        }
                        if y_percent > 0. {
                            let y_pixels = (y_percent * max_h as f64).round() as i32;
                            self.crop_box.bx.resize(
                                self.crop_box.bx.x(),
                                y_pixels / 2,
                                self.crop_box.bx.width(),
                                max_h - y_pixels,
                            );
                        }
                        self.window.redraw();
                    }
                }
            }
        }
    }
}

pub fn gui() {
    let w = 1000;
    let h = 1000;

    let mut config = Config::default();
    let mut gui = Gui::new(w, h, &mut config);
    gui.create_crop_widget(0, 0, 1920, 1080, 0.2);
    gui.main_loop();
}
