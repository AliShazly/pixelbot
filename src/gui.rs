use crate::config::{Bounded, CfgKey, Config, ValType};
use crate::coord::Coord;
use crate::image::{self, image_ops::BlendType, Bgra8, Rgba8, Subpixel};
use crate::input::{get_any_pressed_key, keycode_to_string, wait_for_release};
use crate::logging::{self, drain_log, log, log_err};
use crate::pixel_bot;

use crossbeam::channel;
use fltk::{
    app::{self, *},
    button::*,
    draw,
    enums::*,
    frame::*,
    group::*,
    prelude::*,
    text::{SimpleTerminal, StyleTableEntry, TextBuffer},
    valuator::*,
    window::*,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

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

    pub fn gapify(self, gap: i32) -> Self {
        Self::new(
            self.x + (gap / 2),
            self.y + (gap / 2),
            self.w - gap,
            self.h - gap,
        )
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
struct Graph {
    b: Bounds,
    data_range: Range<i32>,
    points: VecDeque<Coord<i32>>,
    img: image::Image<Vec<u8>, Rgba8>,
    bg_img: image::Image<Vec<u8>, Rgba8>,
    frame: Frame,
    label_frame: Frame,
    redraw: bool,
}

impl Graph {
    pub fn new(b: Bounds, data_range: Range<i32>) -> Self {
        let label_h = (b.h as f32 * 0.05) as i32;
        let frame = Frame::default()
            .with_pos(b.x, b.y)
            .with_size(b.w, b.h - label_h);
        let mut label_frame = Frame::new(frame.x(), frame.y() + frame.h(), b.w, label_h, "")
            .with_align(Align::Left | Align::Inside);

        let buf = vec![0; b.w as usize * b.h as usize * Rgba8::N_SUBPX];
        let graph_img: image::Image<_, Rgba8> =
            image::Image::new(buf.clone(), b.w as usize, b.h as usize);
        let mut bg_img: image::Image<_, Rgba8> = image::Image::new(buf, b.w as usize, b.h as usize);
        bg_img.fill_color(image::Color::new(93, 182, 33, 255));
        bg_img.draw_grid(30, image::Color::new(30, 29, 192, 255));

        label_frame.set_label_font(Font::Courier);
        label_frame.set_label_size(label_h);
        label_frame.set_frame(FrameType::FlatBox);
        label_frame.set_color(Color::Green);

        Self {
            b,
            data_range,
            points: VecDeque::new(),
            img: graph_img,
            bg_img,
            frame,
            label_frame,
            redraw: false,
        }
    }

    fn draw_lines(&mut self) {
        self.img.fill_zeroes();
        self.points.make_contiguous().windows(2).for_each(|coords| {
            let p1 = coords[0];
            let p2 = coords[1];
            self.img.draw_line(
                Coord::new(p1.x as usize, p1.y as usize),
                Coord::new(p2.x as usize, p2.y as usize),
                image::Color::new(255, 0, 0, 255),
            );
        });
        self.img.blend(BlendType::Over, &self.bg_img);
    }

    pub fn draw(&mut self) {
        if self.redraw {
            self.redraw = false;
            self.draw_lines();
            draw::draw_rgba(&mut self.frame, self.img.as_slice()).unwrap();
            self.label_frame.redraw_label();
            self.frame.redraw();
        }
    }

    pub fn tick(&mut self, time: Duration) {
        const INC: i32 = 3;

        let time_norm = clamp(
            1. - ((time.as_millis() as i32 - self.data_range.start) as f32
                / self.data_range.end as f32),
            0.,
            1.,
        );
        let y_scaled = (self.frame.h() - 1) as f32 * time_norm;

        self.points
            .iter_mut()
            .for_each(|c| (*c).x = clamp(c.x + INC, 0, self.b.w - 1));
        self.points.push_back(Coord::new(0, y_scaled as i32));
        if self.points.len() > ((self.b.w - 1) / INC) as usize {
            self.points.pop_front();
        };
        self.label_frame.set_label(&format!("{:?}", time));
        self.redraw = true;
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
    pub fn new(b: Bounds) -> Self {
        let mut bg_box = Group::new(b.x, b.y, b.w, b.h, "");
        bg_box.set_frame(FrameType::FlatBox);
        bg_box.set_color(Color::Red);
        bg_box.end();

        let mut fg_box = Group::new(b.x, b.y, b.w, b.h, "");
        fg_box.set_frame(FrameType::FlatBox);
        fg_box.set_color(Color::Green);
        fg_box.end();

        CropBox {
            bx: fg_box,
            bg_bx: bg_box,
            max_w: b.w,
            max_h: b.h,
            x_offset: b.x,
            y_offset: b.y,
        }
    }

    pub fn change_bounds(&mut self, x_percent: f64, y_percent: f64) {
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

pub struct Gui {
    app: App,
    window: Window,
    config: Arc<RwLock<Config>>,
    init_fns: Vec<Box<dyn FnOnce()>>,
}

impl Gui {
    pub fn new(w: i32, h: i32, config: Arc<RwLock<Config>>) -> Self {
        let app = App::default();
        app::set_visible_focus(false);

        if let Ok(font) = Font::load_font("JetBrainsMono-Medium.ttf") {
            Font::set_font(Font::Courier, &font);
        }
        if let Ok(font) = Font::load_font("JetBrainsMono-Bold.ttf") {
            Font::set_font(Font::CourierBold, &font);
        }
        let window = Window::new(w / 2, h / 2, w, h, "ayooo");

        Self {
            window,
            app,
            config,
            init_fns: Vec::new(),
        }
    }

    pub fn wait(&mut self, dur_secs: f64) -> bool {
        app::sleep(dur_secs);
        self.app.wait()
    }

    pub fn init(
        &mut self,
        screen_w: i32,
        screen_h: i32,
        receiver: channel::Receiver<pixel_bot::Message>,
        cfg_path: &'static str,
    ) {
        // app::background(0, 0, 0);
        // app::foreground(20, 20, 20);

        const GAP: i32 = 10;
        const SLIDER_H: i32 = 50;
        const MIDDLE_OFFSET: i32 = 50;

        let (win_w, win_h) = (self.window.w(), self.window.h());

        // Sliders & crop widget (right side)
        let right_x = (win_w / 2) + MIDDLE_OFFSET;
        let right_y = GAP / 2;
        let right_w = ((win_w - GAP) / 2) - MIDDLE_OFFSET;

        // crop widget
        let crop_box_b =
            self.create_crop_widget(right_x, right_y, screen_w, screen_h, SLIDER_H, right_w);

        // slider group
        let mut cur_slider_b = Bounds::new(right_x, crop_box_b.y + crop_box_b.h, right_w, SLIDER_H);
        let mut slider_grp_b = cur_slider_b;
        CfgKey::iter()
            .filter(|key| !matches!(key, CfgKey::CropW | CfgKey::CropH))
            .filter(|key| !key.is_keycode())
            .for_each(|key| {
                self.create_config_slider(cur_slider_b, key, key.as_string());
                cur_slider_b.y += cur_slider_b.h;
            });
        slider_grp_b.h = cur_slider_b.y - slider_grp_b.y;

        // keycode button group
        let mut button_ev_id = 100;
        let n_keycodes = CfgKey::iter().filter(|k| k.is_keycode()).count() as i32;
        let buttons_y = slider_grp_b.y + slider_grp_b.h;
        let mut cur_but_b = Bounds::new(
            right_x,
            buttons_y + (GAP / 2),
            right_w / n_keycodes,
            ((win_w - buttons_y) / 2) - (GAP / 2),
        );
        let mut but_grp_b = cur_but_b;
        CfgKey::iter()
            .filter(|key| key.is_keycode())
            .for_each(|key| {
                self.create_keycode_but(
                    cur_but_b,
                    key,
                    match key {
                        CfgKey::AimKeycode => "Start Aim".to_string(),
                        CfgKey::ToggleAimKeycode => "Toggle Aim".to_string(),
                        CfgKey::AutoclickKeycode => "Autoclick".to_string(),
                        CfgKey::ToggleAutoclickKeycode => "Cycle Autoclick Mode".to_string(),
                        _ => panic!("Keycode match not exhaustive"),
                    },
                    button_ev_id,
                );
                button_ev_id += 1;
                cur_but_b.x += cur_but_b.w;
            });
        but_grp_b.w = cur_but_b.x - but_grp_b.x;
        self.create_save_config_but(
            Bounds::new(
                right_x,
                (but_grp_b.y + but_grp_b.h) + (GAP / 2),
                right_w,
                but_grp_b.h - (GAP / 2),
            ),
            cfg_path,
        );

        // Screen mirror widget, graph, and terminal (left side)
        let left_w = (win_w / 2) + MIDDLE_OFFSET;
        let left_h = win_h / 3;

        let frm_b = Bounds::new(0, 0, left_w, left_h + GAP).gapify(GAP);
        let mut img_frame = Frame::new(frm_b.x, frm_b.y, frm_b.w, frm_b.h, "");
        let mut img_frame_img =
            image::Image::<Vec<_>, Rgba8>::zeroed(frm_b.w as usize, frm_b.h as usize);
        let mut graph = Graph::new(
            Bounds::new(0, frm_b.y + frm_b.h, left_w, left_h).gapify(GAP),
            5..50,
        );
        let mut term =
            Self::create_term(Bounds::new(0, graph.b.y + graph.b.h, left_w, left_h).gapify(GAP));
        let mut style_buffer = TextBuffer::default();
        let entries: Vec<StyleTableEntry> = vec![
            StyleTableEntry {
                // A
                color: Color::White,
                font: Font::Courier,
                size: 12,
            },
            StyleTableEntry {
                // B
                color: Color::Red,
                font: Font::CourierBold,
                size: 12,
            },
        ];

        let mut now = Instant::now();
        app::add_idle(move || {
            // blinking terminal cursor
            if now.elapsed() > Duration::from_secs_f32(0.5) {
                if term.cursor_color() == Color::Black {
                    term.set_cursor_color(Color::Green);
                    term.redraw();
                } else {
                    term.set_cursor_color(Color::Black);
                    term.redraw();
                }
                now = Instant::now();
            }

            // real ansi codes dont work when I want a font that isnt courier,
            //    so error messages get wrapped in '\x1b' to achieve the same effect using the style buffer
            let log = drain_log();
            if !log.is_empty() {
                let mut flag = true;
                for c in log.chars() {
                    if c == logging::FAKE_ANSI {
                        flag = !flag;
                        continue;
                    }
                    if flag {
                        style_buffer.append("A");
                    } else {
                        style_buffer.append("B");
                    }
                }
                term.append(log.as_str());
                term.set_highlight_data(style_buffer.clone(), entries.clone());
            }

            let msgs: Vec<_> = receiver.try_iter().collect();

            // graph messages
            msgs.iter()
                .filter_map(|msg| match msg {
                    pixel_bot::Message::IterTime(time) => Some(time),
                    _ => None,
                })
                .for_each(|&dur| graph.tick(dur));
            graph.draw();

            // only getting the latest capturedata message
            if let Some(pixel_bot::Message::CaptureData(mut data)) = msgs
                .into_iter()
                .rev()
                .find(|msg| matches!(msg, pixel_bot::Message::CaptureData(_)))
            {
                if let (Some(aim_coord), Some(target_coords)) = (data.aim_coord, data.target_coords)
                {
                    draw_image_overlay(&mut data.img, aim_coord, target_coords);
                }
                let resized = data
                    .img
                    .scale_keep_aspect(img_frame.w() as usize, img_frame.h() as usize);
                img_frame_img.fill_color(image::Color::new(0, 0, 0, 255));
                img_frame_img.layer_image_over(&resized);
                draw::draw_rgba(&mut img_frame, img_frame_img.as_slice()).unwrap();
                img_frame.redraw();
            }
        });

        self.window.end();
        self.window.show();

        let init_fns = std::mem::take(&mut self.init_fns);
        for init_fn in init_fns {
            init_fn();
        }
    }

    fn create_term(b: Bounds) -> SimpleTerminal {
        let mut term = SimpleTerminal::new(b.x, b.y, b.w, b.h, "");
        term.set_selection_color(Color::White);
        term.set_color(Color::Black);
        term.set_cursor_style(fltk::text::Cursor::Simple);
        term.set_scrollbar_size(-1); // no scrollbar
        term.set_ansi(true);
        term
    }

    fn create_crop_widget(
        &mut self,
        x: i32,
        y: i32,
        screen_w: i32,
        screen_h: i32,
        slider_h: i32,
        box_w: i32,
    ) -> Bounds {
        let ratio: f32 = screen_h as f32 / screen_w as f32;
        let box_h = (box_w as f32 * ratio) as i32;
        let crop_box = Rc::new(RefCell::new(CropBox::new(Bounds::new(x, y, box_w, box_h))));

        let slider1_ypos = y + box_h;
        let slider2_ypos = slider1_ypos + slider_h;
        let mut slider1 = self.create_config_slider(
            Bounds::new(x, slider1_ypos, box_w, slider_h),
            CfgKey::CropW,
            CfgKey::CropW.as_string(),
        );
        let mut slider2 = self.create_config_slider(
            Bounds::new(x, slider2_ypos, box_w, slider_h),
            CfgKey::CropH,
            CfgKey::CropH.as_string(),
        );

        let slider1_crop_box = crop_box.clone();
        slider1.set_callback(move |slider| {
            slider1_crop_box
                .borrow_mut()
                .change_bounds(slider.norm_val(), 0.);
        });

        let slider2_crop_box = crop_box.clone();
        slider2.set_callback(move |slider| {
            slider2_crop_box
                .borrow_mut()
                .change_bounds(0., slider.norm_val());
        });

        let (init_x_percent, init_y_percent) = (slider1.norm_val(), slider2.norm_val());
        self.init_fns.push(Box::new(move || {
            crop_box
                .borrow_mut()
                .change_bounds(init_x_percent, init_y_percent);
        }));

        Bounds::new(x, y, box_w, box_h + (slider_h * 2)) // bounds of crop widget
    }

    fn create_save_config_but(&self, b: Bounds, cfg_path: &'static str) {
        let mut button = Button::new(b.x, b.y, b.w, b.h, "_").with_align(Align::Wrap);
        button.set_label_font(Font::CourierBold);

        let mut label = *b"Save config to file";
        let line_w = (b.w / button.measure_label().0) as usize;
        wrap_str(&mut label, line_w);
        button.set_label(&String::from_utf8_lossy(&label));
        button.set_label_size(b.h / 5);

        let config = self.config.clone();
        button.handle(move |_, ev| match ev {
            Event::Released => {
                config.write().unwrap().write_to_file(cfg_path).unwrap();
                log!(
                    "Saved config to {}",
                    match std::path::Path::new(cfg_path).canonicalize() {
                        Ok(abs_path) => abs_path.to_string_lossy().into_owned().split_off(4), // Removing windows extended path prefix
                        Err(_) => cfg_path.to_string(),
                    }
                );
                true
            }
            _ => false,
        });
    }

    fn create_keycode_but(
        &self,
        b: Bounds,
        cfg_key: CfgKey,
        mut label: String,
        ev_id: i32,
    ) -> Button {
        assert!(cfg_key.is_keycode());

        const PUSH: i32 = Event::Push.bits();
        const RELEASE: i32 = Event::Released.bits();
        const TIMEOUT: Duration = Duration::from_secs(5);

        let mut button = Button::new(b.x, b.y, b.w, b.h, "_");
        button.set_label_font(Font::Courier);

        let label_bytes = unsafe { label.as_bytes_mut() };
        let line_w = (b.w / button.measure_label().0) as usize;
        wrap_str(label_bytes, line_w - 3);
        button.set_label(&label);

        let init_val: i32 = self.config.read().unwrap().get(cfg_key).into();
        let full_label = match keycode_to_string(init_val) {
            Ok(string) => format!("{}:\n{}", label, string),
            Err(_) => {
                log_err!(
                    "Config entry `{}` is invalid, using default value",
                    cfg_key.as_string()
                );
                keycode_to_string(cfg_key.default_val().into()).unwrap()
            }
        };
        button.set_label(&full_label);
        button.set_label_font(Font::Courier);

        let mut locked = false;
        let mut start_on_release = false;
        let mut last_released = Instant::now();
        let config = self.config.clone();
        button.handle(move |b, ev| match ev.bits() {
            PUSH => {
                if !locked {
                    locked = true;

                    // need to start capturing keys on release since lmb would get insta detected
                    start_on_release = true;
                }
                true
            }
            RELEASE => {
                if start_on_release {
                    start_on_release = false;
                    last_released = Instant::now();
                    app::handle_main(ev_id).unwrap();
                }
                true
            }
            bits if bits == ev_id => {
                if let Ok(Some(keycode)) = get_any_pressed_key() {
                    match keycode_to_string(keycode) {
                        Ok(name) => {
                            wait_for_release(keycode, Duration::from_millis(500));
                            config
                                .write()
                                .unwrap()
                                .set_val(cfg_key, ValType::Keycode(keycode))
                                .unwrap();
                            b.set_label(&format!("{}:\n{}", label, name));
                        }
                        Err(_) => log_err!("Invalid keycode received: {}", keycode),
                    }
                    locked = false;
                } else if last_released.elapsed() >= TIMEOUT {
                    log!("Key change timeout reached");
                    locked = false;
                } else {
                    app::add_timeout(0.01, move || {
                        // handle_main will fail if called after window is closed
                        let _ = app::handle_main(ev_id);
                    });
                }
                true
            }
            _ => false,
        });
        button
    }

    fn create_config_slider(&self, b: Bounds, cfg_key: CfgKey, label: String) -> HorFillSlider {
        let mut slider = HorFillSlider::new(b.x, b.y, b.w, b.h, "");
        slider.set_color(Color::Dark2);
        slider.set_slider_frame(FrameType::EmbossedBox);

        let val_type = self.config.read().unwrap().get(cfg_key);
        let (cfg_val, bounds_start, bounds_end, precision) = match val_type {
            ValType::Unsigned(ref v) => (
                v.val as f64,
                *v.bounds.start() as f64,
                *v.bounds.end() as f64,
                0,
            ),
            ValType::Float(ref v) => (
                (v.val as f64 * 100.).trunc() / 100.,
                *v.bounds.start() as f64,
                *v.bounds.end() as f64,
                2,
            ),
            _ => panic!("Creating config slider from unbounded value"),
        };

        slider.set_precision(precision);
        slider.set_bounds(bounds_start, bounds_end); // -1 since bounds are inclusive
        slider.set_value(cfg_val);

        let mut label_frame = Frame::new(b.x, b.y, b.w, b.h, "")
            .with_label(format!("{}: {}", label, cfg_val).as_str());
        label_frame.set_label_font(Font::Courier);
        label_frame.set_label_size((b.h as f32 / 2.8) as i32);

        slider.draw(move |slider| {
            label_frame.redraw_label();
            label_frame.set_label(format!("{}: {}", label, slider.value()).as_str());
        });

        let config = self.config.clone();
        slider.handle(move |slider, ev| match ev {
            Event::Released => {
                let val = match &val_type {
                    ValType::Unsigned(_) => {
                        ValType::Unsigned(Bounded::new(slider.value() as u32, 0..=0))
                    }
                    ValType::Float(_) => {
                        ValType::Float(Bounded::new(slider.value() as f32, 0.0..=0.0))
                    }
                    _ => panic!(),
                };

                config.write().unwrap().set_val(cfg_key, val).unwrap();
                true
            }
            _ => false,
        });
        slider
    }
}

fn draw_image_overlay(
    img: &mut image::Image<Vec<u8>, Bgra8>,
    aim_coord: Coord<usize>,
    coord_cluster: Vec<Coord<usize>>,
) {
    let x_max = coord_cluster.iter().max_by_key(|coord| coord.x).unwrap().x;
    let x_min = coord_cluster.iter().min_by_key(|coord| coord.x).unwrap().x;
    let y_max = coord_cluster.iter().max_by_key(|coord| coord.y).unwrap().y;
    let y_min = coord_cluster.iter().min_by_key(|coord| coord.y).unwrap().y;
    let img_center = Coord::new(img.w / 2, img.h / 2);

    img.draw_bbox(
        Coord::new(x_min, y_min),
        x_max - x_min,
        y_max - y_min,
        image::Color::new(0, 255, 0, 255),
    );
    img.draw_crosshair(img_center, 10, image::Color::new(255, 0, 255, 255));
    if img_center.square_dist(aim_coord) > 4 {
        img.draw_crosshair(aim_coord, 10, image::Color::new(255, 0, 0, 255));
        img.draw_line(img_center, aim_coord, image::Color::new(0, 255, 255, 255));
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

fn wrap_str(wrap: &mut [u8], line_w: usize) {
    wrap.split_mut(|c| *c == b'\n').for_each(|substr| {
        let mut last_space_idx = None;
        for (idx, c) in substr.iter().enumerate() {
            if idx > line_w {
                if let Some(space_idx) = last_space_idx {
                    substr[space_idx] = b'\n';
                    wrap_str(substr, line_w);
                }
                break;
            }
            if *c == b' ' {
                last_space_idx = Some(idx);
            }
        }
    })
}
