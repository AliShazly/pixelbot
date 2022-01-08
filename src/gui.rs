use crate::config::{Bounded, CfgKey, Config, ValType};
use crate::coord::Coord;
use crate::image::{self, image_ops::BlendType, Bgra8, Rgba8};
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
use fltk_theme::{SchemeType, WidgetScheme};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};
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
            self.x + gap,
            self.y + gap,
            self.w - (gap * 2),
            self.h - (gap * 2),
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

trait SetLabelWrap {
    fn set_label_wrap(&mut self, label: String, max_w: i32);
}
impl<T> SetLabelWrap for T
where
    T: WidgetExt,
{
    fn set_label_wrap(&mut self, label: String, max_w: i32) {
        const MARGIN: i32 = 0;

        // getting px width of single character
        self.set_label("_");
        let line_w = ((max_w / self.measure_label().0) - MARGIN) as usize;

        // wrapping label string
        let mut label_bytes = label.into_bytes();
        wrap_str_inplace(&mut label_bytes[..], line_w);
        let label = String::from_utf8(label_bytes).unwrap();
        self.set_label(&label);
    }
}

trait SetColorInternal {
    fn set_color_internal(&mut self, color: image::Color<u8>);
}
impl<T> SetColorInternal for T
where
    T: WidgetExt,
{
    fn set_color_internal(&mut self, color: image::Color<u8>) {
        let c = Color::from_rgb(color.r, color.g, color.b);
        self.set_color(c);
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
        let (frame_w, frame_h) = (b.w, b.h - label_h);
        let frame = Frame::new(b.x, b.y, frame_w, frame_h, "");
        let mut label_frame = Frame::new(frame.x(), frame.y() + frame.h(), b.w, label_h, "")
            .with_align(Align::Left | Align::Inside);

        let graph_img = image::Image::<Vec<_>, Rgba8>::zeroed(frame_w as usize, frame_h as usize);
        let mut bg_img = image::Image::<Vec<_>, Rgba8>::zeroed(frame_w as usize, frame_h as usize);
        bg_img.fill_color(image::Color::new(93, 182, 33, 255));
        bg_img.draw_grid(30, image::Color::new(30, 29, 192, 255));

        label_frame.set_label_font(Font::Courier);
        label_frame.set_label_size(label_h - 2 /*small margin*/);
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

            let (frame_w, frame_h) = (self.frame.w() as usize, self.frame.h() as usize);
            if let Some(scaled_img) = self.img.scale_nearest(frame_w, frame_h) {
                self.points.clear();
                self.img = scaled_img;
                self.bg_img =
                    image::Image::<Vec<_>, Rgba8>::zeroed(frame_w as usize, frame_h as usize);
                self.bg_img.fill_color(image::Color::new(93, 182, 33, 255));
                self.bg_img
                    .draw_grid(30, image::Color::new(30, 29, 192, 255));
            }

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
            .for_each(|c| (*c).x = clamp(c.x + INC, 0, self.frame.w() - 1));
        self.points.push_back(Coord::new(0, y_scaled as i32));
        if self.points.len() > ((self.frame.w() - 1) / INC) as usize {
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

        CropBox { bx: fg_box, bg_bx: bg_box }
    }

    pub fn change_bounds(&mut self, x_percent: f64, y_percent: f64) {
        if x_percent > 0. {
            let x_pixels = (x_percent * self.bg_bx.w() as f64).round() as i32;
            let box_y = self.bx.y();
            let box_h = self.bx.height();
            self.bx.resize(
                (x_pixels / 2) + self.bg_bx.x(),
                box_y,
                self.bg_bx.w() - x_pixels,
                box_h,
            );
        }
        if y_percent > 0. {
            let y_pixels = (y_percent * self.bg_bx.h() as f64).round() as i32;
            let box_x = self.bx.x();
            let box_w = self.bx.width();
            self.bx.resize(
                box_x,
                (y_pixels / 2) + self.bg_bx.y(),
                box_w,
                self.bg_bx.h() - y_pixels,
            );
        }
        self.bg_bx.redraw();
        self.bx.redraw();
    }
}

struct ResponsiveButton {
    b: Bounds,
    button: Button,
    push_event: i32,
    release_event: i32,
}

impl ResponsiveButton {
    fn new(bnds: Bounds, label: String, font: Font) -> Self {
        let button_released = unique_event_id();
        let button_pushed = unique_event_id();
        let fade_out = unique_event_id();

        // group handles color changes
        let mut draw_frame = Frame::new(bnds.x, bnds.y, bnds.w, bnds.h, "");
        let mut grp = Group::new(bnds.x, bnds.y, bnds.w, bnds.h, "");
        grp.set_frame(FrameType::RFlatBox);
        draw_frame.set_frame(FrameType::FlatBox);

        let mut button = Button::new(bnds.x, bnds.y, bnds.w, bnds.h, "").with_align(Align::Wrap);
        button.set_frame(FrameType::NoBox);
        button.set_down_frame(FrameType::NoBox);
        button.set_label_font(font);

        grp.end();

        let (r, g, b) = Color::Background.to_rgb();
        let idle_color = image::Color::new(r, g, b, 255);

        let (r, g, b) = Color::Background.darker().to_rgb();
        let hover_color = image::Color::new(r, g, b, 255);

        let (r, g, b) = Color::Background.darker().darker().to_rgb();
        let push_color = image::Color::new(r, g, b, 255);

        const LERP_INC: f32 = 1. / 10.;
        const ITER_TIME: f64 = 1. / 144.;
        let mut fade_color = idle_color;
        let mut fade_lerp = 0.;
        let mut continue_fading_out = false;
        grp.handle(move |g, ev| match ev {
            Event::Enter => {
                draw::set_cursor(Cursor::Hand);
                if continue_fading_out {
                    continue_fading_out = false;
                }
                g.set_color_internal(hover_color);
                draw_frame.redraw();
                g.redraw();
                true
            }
            Event::Leave => {
                draw::set_cursor(Cursor::Default);
                continue_fading_out = true;
                fade_color = idle_color;
                app::handle_main(fade_out).unwrap();
                true
            }
            _ if ev.bits() == fade_out => {
                if fade_lerp > (1. + f32::EPSILON) {
                    continue_fading_out = false;
                }
                if !continue_fading_out {
                    fade_lerp = 0.;
                    return true;
                }

                let (cur_r, cur_g, cur_b) = g.color().to_rgb();
                let current_color = image::Color::new(cur_r, cur_g, cur_b, 255);
                let faded_color = current_color.lerp(fade_color, fade_lerp);
                fade_lerp += LERP_INC;
                g.set_color_internal(faded_color);
                draw_frame.redraw();
                g.redraw();
                app::add_timeout3(ITER_TIME, move |_| {
                    let _ = app::handle_main(fade_out);
                });
                true
            }
            _ if ev.bits() == button_pushed => {
                g.set_color_internal(push_color);
                draw_frame.redraw();
                g.redraw();
                true
            }
            _ if ev.bits() == button_released => {
                g.set_color_internal(hover_color);
                draw_frame.redraw();
                g.redraw();
                true
            }
            _ => false,
        });

        let mut ret = Self {
            b: bnds,
            button,
            push_event: button_pushed,
            release_event: button_released,
        };
        ret.button.set_label_wrap(label, ret.button.width());
        ret
    }
}

pub struct Gui {
    app: App,
    window: Window,
    config: Arc<RwLock<Config>>,

    // we don't want multiple keycode buttons searching for input concurrently
    capture_input_lock: Rc<Cell<bool>>,
}

impl Gui {
    pub fn new(w: i32, h: i32, config: Arc<RwLock<Config>>) -> Self {
        let app = App::default();
        let scheme = WidgetScheme::new(SchemeType::SvgBased);
        scheme.apply();
        app::set_visible_focus(false);
        app::set_frame_type(FrameType::RFlatBox);

        if let Ok(font) = Font::load_font("JetBrainsMono-Medium.ttf") {
            Font::set_font(Font::Courier, &font);
        }
        if let Ok(font) = Font::load_font("JetBrainsMono-Bold.ttf") {
            Font::set_font(Font::CourierBold, &font);
        }
        let window = Window::new(w / 2, h / 2, w, h, "ayooo");

        let capture_input_lock = Rc::new(Cell::new(false));

        Self {
            window,
            app,
            config,
            capture_input_lock,
        }
    }

    pub fn wait(&mut self, dur_secs: f64) -> bool {
        app::sleep(dur_secs);
        self.app.wait()
    }

    pub fn init(
        &mut self,
        screen_aspect_ratio: f32,
        receiver: channel::Receiver<pixel_bot::Message>,
        cfg_path: &'static str,
    ) {
        // app::background(0, 0, 0);
        // app::foreground(20, 20, 20);
        self.window.make_resizable(true);
        self.window.size_range(800, 700, 3480, 2160);

        const GAP: i32 = 5;
        const SLIDER_H: i32 = 50;
        const MIDDLE_OFFSET: i32 = 50;

        let (win_w, win_h) = (self.window.w(), self.window.h());

        // Sliders & crop widget (right side)
        let right_x = (win_w / 2) + MIDDLE_OFFSET;
        let right_y = GAP;
        let right_w = ((win_w - (GAP * 2)) / 2) - MIDDLE_OFFSET;

        // crop widget
        let crop_box_b = self.create_crop_widget(
            right_x,
            right_y,
            screen_aspect_ratio,
            SLIDER_H,
            right_w,
            GAP,
        );

        // slider group
        let mut cur_slider_b = Bounds::new(
            right_x,
            crop_box_b.y + crop_box_b.h + GAP,
            right_w,
            SLIDER_H,
        );
        let mut slider_grp_b = cur_slider_b;
        CfgKey::iter()
            .filter(|key| !matches!(key, CfgKey::CropW | CfgKey::CropH))
            .filter(|key| matches!(key.default_val(), ValType::Unsigned(_) | ValType::Float(_)))
            .for_each(|key| {
                self.create_config_slider(cur_slider_b, key, key.as_string());
                cur_slider_b.y += cur_slider_b.h + GAP;
            });
        slider_grp_b.h = cur_slider_b.y - slider_grp_b.y;

        // keycode button group
        let buttons_y = slider_grp_b.y + slider_grp_b.h;
        self.create_cfg_button_group(
            Bounds::new(right_x, buttons_y, right_w, (win_w - buttons_y) - GAP),
            3,
            cfg_path,
        );

        // Screen mirror widget, graph, and terminal (left side)
        let left_w = (win_w / 2) + MIDDLE_OFFSET;
        let left_h = win_h / 3;

        let frm_b = Bounds::new(0, 0, left_w, left_h + (GAP * 2)).gapify(GAP);
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
        app::add_idle3(move |_| {
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
            if let Some(pixel_bot::Message::CaptureData(data)) = msgs
                .into_iter()
                .rev()
                .find(|msg| matches!(msg, pixel_bot::Message::CaptureData(_)))
            {
                let (frame_w, frame_h) = (img_frame.w() as usize, img_frame.h() as usize);
                let (old_w, old_h) = (data.img.w, data.img.h);
                let mut resized_data_img = match data.img.scale_keep_aspect(frame_w, frame_h) {
                    Some(resized) => resized,
                    None => data.img,
                };

                if let (Some(mut aim_coord), Some(mut target_coords)) =
                    (data.aim_coord, data.target_coords)
                {
                    // scaling coords by resize ratio
                    let ratio = Coord::new(
                        resized_data_img.w as f32 / old_w as f32,
                        resized_data_img.h as f32 / old_h as f32,
                    );

                    aim_coord = Coord::new(
                        (aim_coord.x as f32 * ratio.x) as usize,
                        (aim_coord.y as f32 * ratio.y) as usize,
                    );

                    target_coords.iter_mut().for_each(|coord| {
                        coord.x = (coord.x as f32 * ratio.x) as usize;
                        coord.y = (coord.y as f32 * ratio.y) as usize;
                    });

                    draw_image_overlay(&mut resized_data_img, aim_coord, target_coords);
                }

                if let Some(resized_bg) = img_frame_img.scale_nearest(frame_w, frame_h) {
                    img_frame_img = resized_bg;
                }

                img_frame_img.fill_color(image::Color::new(0, 0, 0, 255));
                img_frame_img.layer_image_over(&resized_data_img);

                draw::draw_rgba(&mut img_frame, img_frame_img.as_slice()).unwrap();
                img_frame.redraw();
            }
        });

        self.window.end();
        self.window.show();
    }

    fn create_term(b: Bounds) -> SimpleTerminal {
        let mut term = SimpleTerminal::new(b.x, b.y, b.w, b.h, "");
        term.set_selection_color(Color::White);
        term.set_color(Color::Black);
        term.set_cursor_style(fltk::text::Cursor::Simple);
        term.set_scrollbar_size(-1); // no scrollbar
        term.set_ansi(true);
        term.set_frame(app::frame_type());
        term
    }

    fn create_cfg_button_group(&self, b: Bounds, row_len: i32, cfg_save_path: &'static str) {
        let pretty_name = |key: CfgKey| match key {
            CfgKey::AimKeycode => "Start Aim".to_string(),
            CfgKey::ToggleAimKeycode => "Toggle Aim".to_string(),
            CfgKey::AutoclickKeycode => "Autoclick".to_string(),
            CfgKey::ToggleAutoclickKeycode => "Cycle Autoclick Mode".to_string(),
            CfgKey::FakeLmbKeycode => "Fake Lmb".to_string(),
            _ => panic!("Keycode match not exhaustive"),
        };
        let n_buttons = CfgKey::iter().filter(|k| k.is_keycode()).count() as i32;

        let button_w = b.w / row_len;
        let button_h = b.h / ((button_w * n_buttons) as f32 / b.w as f32).ceil() as i32;

        let mut current_bounds = Bounds::new(b.x, b.y, button_w, button_h);
        for key in CfgKey::iter().filter(|k| k.is_keycode()) {
            self.create_keycode_but(current_bounds, key, pretty_name(key));
            current_bounds.x += button_w;
            if current_bounds.x + button_w > b.x + b.w {
                current_bounds.x = b.x;
                current_bounds.y += button_h;
            }
        }
        self.create_save_config_but(current_bounds, cfg_save_path);
    }

    fn create_crop_widget(
        &mut self,
        x: i32,
        y: i32,
        aspect_ratio: f32,
        slider_h: i32,
        box_w: i32,
        slider_gap: i32,
    ) -> Bounds {
        let box_h = (box_w as f32 * aspect_ratio) as i32;
        let crop_box = Rc::new(RefCell::new(CropBox::new(Bounds::new(x, y, box_w, box_h))));

        let slider1_ypos = y + box_h + slider_gap;
        let slider2_ypos = slider1_ypos + slider_h + slider_gap;
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
        crop_box
            .borrow_mut()
            .change_bounds(init_x_percent, init_y_percent);

        Bounds::new(x, y, box_w, box_h + (slider_h * 2) + (slider_gap * 2))
    }

    fn create_save_config_but(&self, b: Bounds, cfg_path: &'static str) {
        let ResponsiveButton {
            b: _,
            mut button,
            push_event: button_pushed,
            release_event: button_released,
        } = ResponsiveButton::new(b, "Save config to file".to_string(), Font::CourierBold);

        let config = self.config.clone();
        button.handle(move |_, ev| match ev {
            Event::Push => {
                app::handle_main(button_pushed).unwrap();
                true
            }
            Event::Released => {
                app::handle_main(button_released).unwrap();
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

    fn create_keycode_but(&self, b: Bounds, cfg_key: CfgKey, label: String) -> Button {
        assert!(cfg_key.is_keycode());

        let capture_input = unique_event_id();

        let init_keycode: i32 = self.config.read().unwrap().get(cfg_key).into();
        let init_string = match keycode_to_string(init_keycode) {
            Ok(string) => string,
            Err(_) => {
                log_err!(
                    "Config entry `{}` is invalid, using default value",
                    cfg_key.as_string()
                );
                keycode_to_string(cfg_key.default_val().into()).unwrap()
            }
        };

        let ResponsiveButton {
            b: _,
            mut button,
            push_event: button_pushed,
            release_event: button_released,
        } = ResponsiveButton::new(b, "".to_string(), Font::Courier);

        // Label frames
        let labels_gap = (b.h as f32 * 0.35) as i32;
        let (center_x, center_y) = (b.x + (b.w / 2), b.y + (b.h / 2));
        let mut name_label =
            Frame::new(center_x, center_y - (labels_gap / 2), 0, 0, "").with_align(Align::Center);
        let val_label = Rc::new(RefCell::new(
            Frame::new(center_x, center_y + (labels_gap / 2), 0, 0, "").with_align(Align::Center),
        ));

        name_label.set_label_font(Font::Courier);
        name_label.set_label_wrap(format!("{}:", label), button.width());
        val_label.borrow_mut().set_label_font(Font::CourierBold);
        val_label
            .borrow_mut()
            .set_label(&format!("'{}'", init_string));

        let val_label_clone = val_label.clone();
        button.draw(move |_| {
            val_label_clone.borrow_mut().redraw_label();
            name_label.redraw_label();
        });

        const TIMEOUT: Duration = Duration::from_secs(5);
        let mut start_on_release = false;
        let mut last_released = Instant::now();
        let mut last_label = String::new();
        let config = self.config.clone();
        let locked = self.capture_input_lock.clone();
        button.handle(move |but, ev| match ev {
            Event::Push => {
                if !locked.get() {
                    locked.set(true);
                    app::handle_main(button_pushed).unwrap();

                    // need to start capturing keys on release since lmb would get insta detected
                    start_on_release = true;
                }
                true
            }
            Event::Released => {
                app::handle_main(button_released).unwrap();
                if start_on_release {
                    start_on_release = false;
                    last_label = val_label.borrow().label();
                    val_label
                        .borrow_mut()
                        .set_label_wrap("Press any key...".to_string(), but.width());
                    but.redraw();
                    last_released = Instant::now();
                    app::handle_main(capture_input).unwrap();
                }
                true
            }
            _ if ev.bits() == capture_input => {
                if let Ok(Some(keycode)) = get_any_pressed_key() {
                    match keycode_to_string(keycode) {
                        Ok(keycode_string) => {
                            wait_for_release(keycode, Duration::from_millis(500));
                            config
                                .write()
                                .unwrap()
                                .set_val(cfg_key, ValType::Keycode(keycode))
                                .unwrap();
                            val_label
                                .borrow_mut()
                                .set_label(&format!("'{}'", keycode_string));
                            but.redraw();
                        }
                        Err(_) => log_err!("Invalid keycode received: {}", keycode),
                    }
                    locked.set(false);
                } else if last_released.elapsed() >= TIMEOUT {
                    log!("Key change timeout reached");
                    val_label.borrow_mut().set_label(&last_label);
                    but.redraw();
                    locked.set(false);
                } else {
                    app::add_timeout3(0.01, move |_| {
                        // handle_main will fail if called after window is closed
                        let _ = app::handle_main(capture_input);
                    });
                }
                true
            }
            _ => false,
        });
        button
    }

    fn create_config_slider(&self, b: Bounds, cfg_key: CfgKey, label: String) -> HorFillSlider {
        let mut draw_frame = Frame::new(b.x, b.y, b.w, b.h, "");
        let mut slider = HorFillSlider::new(b.x, b.y, b.w, b.h, "");
        draw_frame.set_frame(FrameType::FlatBox);
        slider.set_color(Color::Dark2);
        slider.set_selection_color(Color::Green);
        slider.set_frame(app::frame_type());

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
                (*v.bounds.start() as f64 * 100.).ceil() / 100.,
                (*v.bounds.end() as f64 * 100.).floor() / 100.,
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
            draw_frame.redraw();
        });

        let config = self.config.clone();
        slider.handle(move |slider, ev| match ev {
            Event::Released => {
                let val = match val_type {
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

fn wrap_str_inplace(wrap: &mut [u8], line_w: usize) {
    wrap.split_mut(|c| *c == b'\n').for_each(|substr| {
        let mut last_space_idx = None;
        for (idx, c) in substr.iter().enumerate() {
            if idx > line_w {
                if let Some(space_idx) = last_space_idx {
                    substr[space_idx] = b'\n';
                    wrap_str_inplace(substr, line_w);
                }
                break;
            }
            if *c == b' ' {
                last_space_idx = Some(idx);
            }
        }
    })
}

fn unique_event_id() -> i32 {
    static EVENT_ID: AtomicI32 = AtomicI32::new(100);
    EVENT_ID.fetch_add(1, Ordering::Relaxed)
}
