use crate::config::{CfgKey, CfgValue, Config};
use crate::coord::Coord;
use crate::image::{self, image_ops::BlendType, Rgba8, Subpixel};

use fltk::{
    app::{self, *},
    draw::*,
    enums::*,
    frame::*,
    group::*,
    prelude::*,
    valuator::*,
    window::*,
};
use std::assert;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{mpsc, Arc, RwLock};
use std::time::{Duration, Instant};

pub enum Message {
    ChangedConfig,
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Bounds {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
}

struct Graph {
    b: Bounds,
    receiver: mpsc::Receiver<Duration>,
    data_range: Range<i32>,
    points: VecDeque<Coord<i32>>,
    img: image::Image<Vec<<Rgba8 as Subpixel>::Inner>, Rgba8>,
    current_time: Instant,
}

impl Graph {
    fn new(b: Bounds, receiver: mpsc::Receiver<Duration>, data_range: Range<i32>) -> Self {
        Self {
            b,
            receiver,
            data_range,
            points: VecDeque::new(),
            img: image::Image::new(
                vec![0; b.w as usize * b.h as usize * Rgba8::N_SUBPX],
                b.w as usize,
                b.h as usize,
            ),
            current_time: Instant::now(),
        }
    }

    pub fn tick(&mut self) -> Option<(Vec<Coord<i32>>, Duration)> {
        const INC: i32 = 1;
        const TICK_FREQ: Duration = Duration::from_millis(1);

        if self.current_time.elapsed() < TICK_FREQ {
            return None;
        }

        if let Some(time) = self.receiver.try_iter().last() {
            let time_norm = clamp(
                1. - ((time.as_millis() as i32 - self.data_range.start) as f32
                    / self.data_range.end as f32),
                0.,
                1.,
            );
            let y_scaled = (self.b.h - 1) as f32 * time_norm;

            self.points.iter_mut().for_each(|c| (*c).x += INC);
            self.points.push_back(Coord::new(0, y_scaled as i32));
            if self.points.len() > self.b.w as usize {
                self.points.pop_front();
            };
            self.current_time = Instant::now();
            Some((self.points.iter().copied().collect(), time))
        } else {
            None
        }
    }
}

#[derive(Debug)]
struct CropBox {
    bg_bx: Group,
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
        self.bg_bx.redraw();
        self.bx.redraw();
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
    window: Window,
    crop_box: Rc<RefCell<CropBox>>,
    config: Arc<RwLock<Config>>,
    sender: mpsc::Sender<Message>,
    init_fns: Vec<Box<dyn Fn()>>,
}

impl Gui {
    pub fn new(w: i32, h: i32, config: Arc<RwLock<Config>>, sender: mpsc::Sender<Message>) -> Self {
        let app = App::default();
        app::set_visible_focus(false);
        let window = Window::new(w / 2, h / 2, w, h, "ayooo");

        Self {
            w,
            h,
            window,
            app,
            crop_box: Rc::new(RefCell::new(CropBox {
                bx: Group::default_fill(),
                bg_bx: Group::default_fill(),
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

    pub fn create_graph(
        &self,
        b: Bounds,
        receiver: mpsc::Receiver<Duration>,
        graph_range_ms: Range<i32>,
        color: image::Color<u8>,
    ) {
        const LABEL_SIZE_DIVISOR: i32 = 15;
        let mut frm = Frame::default().with_pos(b.x, b.y).with_size(b.w, b.h);
        let mut label_frame = Frame::new(b.x, b.y + b.h, b.w, b.h / (LABEL_SIZE_DIVISOR / 2), "")
            .with_label("ayooooo")
            .with_align(Align::LeftBottom | Align::Inside);
        label_frame.set_label_font(Font::HelveticaBold);
        label_frame.set_label_size(b.h / LABEL_SIZE_DIVISOR);
        label_frame.set_frame(FrameType::FlatBox);
        label_frame.set_color(Color::Green);

        let mut graph = Graph::new(
            Bounds::new(frm.x(), frm.y(), frm.w(), frm.h()),
            receiver,
            graph_range_ms,
        );

        let buf_size = b.w as usize * b.h as usize * Rgba8::N_SUBPX;
        let mut graph_img: image::Image<_, Rgba8> =
            image::Image::new(vec![0u8; buf_size], b.w as usize, b.h as usize);
        let mut bg_img: image::Image<_, Rgba8> =
            image::Image::new(vec![0u8; buf_size], b.w as usize, b.h as usize);

        bg_img.fill_color(image::Color::new(0, 0, 255, 255));
        bg_img.draw_grid(30, image::Color::new(0, 255, 0, 255));

        app::add_idle(move || {
            if let Some((coords, time)) = graph.tick() {
                label_frame.set_label(format!("{:?}", time).as_str());
                label_frame.redraw_label();
                frm.redraw();
                coords.windows(2).for_each(|coords| {
                    let p1 = coords[0];
                    let p2 = coords[1];
                    graph_img.draw_line(Coord::new(p1.y, p1.x), Coord::new(p2.y, p2.x), color);
                });
                graph_img.blend(BlendType::Over, &bg_img);
                draw_rgba(&mut frm, graph_img.as_slice()).unwrap();
                graph_img.fill_zeroes();
            }
        });
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
        let mut label_frame = Frame::new(x, y, w, h, "").with_label(label);
        const LABEL_SIZE_DIVISOR: f32 = 2.8;
        label_frame.set_label_font(Font::HelveticaBold);
        label_frame.set_label_size((h as f32 / LABEL_SIZE_DIVISOR) as i32);

        slider.set_color(Color::Dark2);
        slider.set_slider_frame(FrameType::EmbossedBox);

        let cfg_value = self.config.read().unwrap().get(cfg_key);
        let val_bounds = cfg_value.bounds.unwrap();
        slider.set_value(cfg_value.val);
        slider.set_bounds(val_bounds.start, val_bounds.end - 1.); // -1 since bounds are inclusive
        slider.set_precision(2);

        // To make sure the slider label is always drawn on top of the slider
        slider.draw(move |_| {
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

    pub fn create_crop_widget(&mut self, b: Bounds, scalar: f32) {
        assert!(std::ops::RangeInclusive::new(0., 1.).contains(&scalar));

        let box_w = (b.w as f32 * scalar) as i32;
        let box_h = (b.h as f32 * scalar) as i32;
        let slider_h = self.h / 20;
        let slider1_ypos = b.y + box_h;
        let slider2_ypos = slider1_ypos + slider_h;

        let mut bg_box = Group::new(b.x, b.y, box_w, box_h, "");
        bg_box.set_frame(FrameType::FlatBox);
        bg_box.set_color(Color::Red);
        bg_box.end();

        let mut fg_box = Group::new(b.x, b.y, box_w, box_h, "");
        fg_box.set_frame(FrameType::FlatBox);
        fg_box.set_color(Color::Green);
        fg_box.end();

        self.crop_box = Rc::new(RefCell::new(CropBox {
            bx: fg_box,
            bg_bx: bg_box,
            max_w: box_w,
            max_h: box_h,
            x_offset: b.x,
            y_offset: b.y,
        }));

        let mut slider1 =
            self.cfg_slider(CfgKey::CropW, b.x, slider1_ypos, box_w, slider_h, "Crop X");
        let mut slider2 =
            self.cfg_slider(CfgKey::CropH, b.x, slider2_ypos, box_w, slider_h, "Crop Y");

        let crop_box = self.crop_box.clone();
        slider1.set_callback(move |slider| {
            crop_box
                .try_borrow_mut()
                .unwrap()
                .change_bounds(slider.norm_val(), 0.);
        });

        let crop_box = self.crop_box.clone();
        slider2.set_callback(move |slider| {
            crop_box
                .try_borrow_mut()
                .unwrap()
                .change_bounds(0., slider.norm_val());
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
        // app::background(0, 0, 0); // background color. For input/output and text widgets, use app::background2
        // app::foreground(20, 20, 20); // labels
        // app::set_font(Font::Courier);
        // app::set_font_size(16);
        // app::set_frame_type(FrameType::RFlatBox);

        self.window.end();
        self.window.show();
        for init_fn in self.init_fns.iter() {
            init_fn();
        }
    }

    pub fn wait(&mut self) -> bool {
        self.app.wait()
    }
}

fn clamp<T>(val: T, min: T, max: T) -> T
where
    T: std::cmp::PartialOrd + Copy,
{
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}
