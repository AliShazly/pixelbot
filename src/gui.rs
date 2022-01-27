use crate::config::{Bounded, CfgKey, Config, ValType};
use crate::coord::Coord;
use crate::image::{self, image_ops::BlendType, Bgra8, Rgba8};
use crate::input::{get_any_pressed_key, keycode_to_string, wait_for_release};
use crate::logging::{self, drain_log, log, log_err};
use crate::pixel_bot;

use crossbeam::channel;
use fltk::{
    app::{self, App},
    button::Button,
    draw,
    enums::{Align, Color, Cursor, Event, Font, FrameType, Key},
    frame::Frame,
    group::Group,
    prelude::*,
    text::{SimpleTerminal, StyleTableEntry, TextBuffer},
    valuator::HorFillSlider,
    window::Window,
};
use rand::seq::IteratorRandom;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

struct Palette;
impl Palette {
    const BG0_H: Color = Color::from_hex(0x1d2021);
    const BG0: Color = Color::from_hex(0x282828);
    const BG1: Color = Color::from_hex(0x3c3836);
    const GRAY: Color = Color::from_hex(0x928374);
    const FG0: Color = Color::from_hex(0xfbf1c7);
    const FG1: Color = Color::from_hex(0xebdbb2);
    const FG2: Color = Color::from_hex(0xd5c4a1);

    const RED: Color = Color::from_hex(0xfb4934);
    const GREEN: Color = Color::from_hex(0xb8bb26);
    const YELLOW: Color = Color::from_hex(0xfabd2f);
    const BLUE: Color = Color::from_hex(0x83a598);
    const PURPLE: Color = Color::from_hex(0xd3869b);
    const AQUA: Color = Color::from_hex(0x8ec07c);
    const ORANGE: Color = Color::from_hex(0xfe8019);

    const COLORS: [Color; 7] = [
        Self::RED,
        Self::GREEN,
        Self::YELLOW,
        Self::BLUE,
        Self::PURPLE,
        Self::AQUA,
        Self::ORANGE,
    ];
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

    pub fn gapify(&self, gap: i32) -> Self {
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
        (self.value() - self.minimum()) / (self.maximum() - self.minimum())
    }
}

trait SetLabelWrap {
    fn set_label_wrap(&mut self, label: String, max_w: i32);
}
impl<T> SetLabelWrap for T
where
    T: WidgetBase,
{
    fn set_label_wrap(&mut self, label: String, max_w: i32) {
        const MARGIN: i32 = 5;

        // getting px width of single character
        self.set_label("_");
        let line_w = ((max_w / self.measure_label().0) - MARGIN) as usize;

        // wrapping label string
        let mut label_bytes = label.into_bytes();
        wrap_str_inplace(&mut label_bytes[..], line_w);
        let label = String::from_utf8_lossy(&label_bytes);
        self.set_label(&label);
    }
}

trait InternalColorConvert {
    fn from_internal(color: image::Color<u8>) -> Self;
    fn to_internal(&self) -> image::Color<u8>;
}
impl InternalColorConvert for Color {
    fn from_internal(color: image::Color<u8>) -> Self {
        Self::from_rgb(color.r, color.g, color.b)
    }
    fn to_internal(&self) -> image::Color<u8> {
        let (r, g, b) = self.to_rgb();
        image::Color::new(r, g, b, 255)
    }
}

struct Graph<const CIRC_BUF_SIZE: usize> {
    b: Bounds,
    data_range: Range<i32>,
    points: VecDeque<Coord<i32>>,
    img: image::Image<Vec<u8>, Rgba8>,
    bg_img: image::Image<Vec<u8>, Rgba8>,
    frame: Frame,
    label_frame: Frame,
    redraw: bool,
    rolling_avg_buf: [Duration; CIRC_BUF_SIZE],
    rolling_avg_idx: usize,
}

impl<const CIRC_BUF_SIZE: usize> Graph<CIRC_BUF_SIZE> {
    pub fn new(b: Bounds, data_range: Range<i32>) -> Self {
        let label_h = (b.h as f32 * 0.05) as i32;
        let (frame_w, frame_h) = (b.w, b.h - label_h);
        let frame = Frame::new(b.x, b.y, frame_w, frame_h, "");
        let mut label_frame = Frame::new(frame.x(), frame.y() + frame.h(), b.w, label_h, "")
            .with_align(Align::Left | Align::Inside);

        let graph_img = image::zeroed::<Rgba8>(frame_w as usize, frame_h as usize);
        let mut bg_img = image::zeroed::<Rgba8>(frame_w as usize, frame_h as usize);
        bg_img.fill_color(Palette::BG0.to_internal());
        bg_img.draw_grid(30, Palette::AQUA.to_internal());

        label_frame.set_label_font(Font::Courier);
        label_frame.set_label_size(label_h - 2 /*small margin*/);
        label_frame.set_frame(FrameType::FlatBox);
        label_frame.set_color(Palette::BG0_H);

        Self {
            b,
            data_range,
            points: VecDeque::new(),
            img: graph_img,
            bg_img,
            frame,
            label_frame,
            redraw: false,
            rolling_avg_buf: [Duration::default(); CIRC_BUF_SIZE],
            rolling_avg_idx: 0,
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
                Palette::RED.to_internal(),
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
                self.bg_img = image::zeroed::<Rgba8>(frame_w as usize, frame_h as usize);
                self.bg_img.fill_color(Palette::BG0.to_internal());
                self.bg_img.draw_grid(30, Palette::AQUA.to_internal());
            }

            self.draw_lines();
            draw::draw_rgba(&mut self.frame, self.img.as_slice()).unwrap();
            self.label_frame.redraw_label();
            self.frame.redraw();
        }
    }

    pub fn tick(&mut self, single_time: Duration) {
        const INC: i32 = 3;

        self.rolling_avg_idx = (self.rolling_avg_idx + 1) % CIRC_BUF_SIZE;
        self.rolling_avg_buf[self.rolling_avg_idx] = single_time;
        let avg_time = self.rolling_avg_buf.iter().sum::<Duration>() / CIRC_BUF_SIZE as u32;

        let time_norm = clamp(
            1. - ((avg_time.as_millis() as i32 - self.data_range.start) as f32
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
        self.label_frame.set_label(&format!(
            "Frame time: {:.2}ms | FPS: {:.0}",
            avg_time.as_secs_f32() * 1000.,
            1. / avg_time.as_secs_f32()
        ));

        self.redraw = true;
    }
}

#[derive(Debug)]
struct CropBox {
    bg_bx: Group,
    bx: Rc<RefCell<Group>>,
    ratio_cache: Rc<Cell<(f32, f32)>>, // last valid w/h ratios
}

impl CropBox {
    pub fn new(b: Bounds) -> Self {
        let mut draw_frame = Frame::new(b.x, b.y, b.w, b.h, "");
        draw_frame.set_frame(FrameType::FlatBox);

        let mut bg_box = Group::new(b.x, b.y, b.w, b.h, "");
        bg_box.set_frame(app::frame_type());
        bg_box.set_color(Palette::BG0);
        bg_box.end();

        let mut fg_box = Group::new(b.x, b.y, b.w, b.h, "");
        fg_box.set_frame(app::frame_type());
        fg_box.set_color(Palette::GREEN);
        fg_box.end();

        bg_box.draw(move |_| {
            draw_frame.redraw();
        });

        // the fg box behaves extremely wack when we let fltk handle the resizing
        // since the bg box behaves correctly, we just resize the fg box according to the bg box's previous proportions
        let ratio_cache = Rc::new(Cell::new((1., 1.)));
        let ratio_cache_clone = ratio_cache.clone();
        let fg_box_rc = Rc::new(RefCell::new(fg_box));
        let fg_box_rc_clone = fg_box_rc.clone();
        bg_box.handle(move |bx, ev| match ev {
            Event::Resize => {
                let ratios = ratio_cache_clone.get();
                let new_w = (bx.w() as f32 * ratios.0) as i32;
                let new_h = (bx.h() as f32 * ratios.1) as i32;

                fg_box_rc_clone.borrow_mut().resize(
                    bx.x() + ((bx.w() / 2) - (new_w / 2)),
                    bx.y() + ((bx.h() / 2) - (new_h / 2)),
                    new_w,
                    new_h,
                );
                true
            }
            _ => false,
        });

        CropBox {
            bx: fg_box_rc,
            bg_bx: bg_box,
            ratio_cache,
        }
    }

    pub fn change_bounds(&mut self, x_percent: f64, y_percent: f64) {
        let mut bx = self.bx.borrow_mut();

        if x_percent > 0. {
            let x_pixels = (x_percent * self.bg_bx.w() as f64).round() as i32;
            let box_y = bx.y();
            let box_h = bx.height();
            bx.resize(
                (x_pixels / 2) + self.bg_bx.x(),
                box_y,
                self.bg_bx.w() - x_pixels,
                box_h,
            );
        }
        if y_percent > 0. {
            let y_pixels = (y_percent * self.bg_bx.h() as f64).round() as i32;
            let box_x = bx.x();
            let box_w = bx.width();
            bx.resize(
                box_x,
                (y_pixels / 2) + self.bg_bx.y(),
                box_w,
                self.bg_bx.h() - y_pixels,
            );
        }

        self.ratio_cache.set((
            bx.w() as f32 / self.bg_bx.w() as f32,
            bx.h() as f32 / self.bg_bx.h() as f32,
        ));
        self.bg_bx.redraw();
        bx.redraw();
    }
}

struct ResponsiveButton {
    b: Bounds,
    button: Button,
    push_event: i32,
    release_event: i32,
}

impl ResponsiveButton {
    fn new(bnds: Bounds, label: String, font: Font, init_color: Color) -> Self {
        let button_released = unique_event_id();
        let button_pushed = unique_event_id();
        let fade = unique_event_id();

        let mut draw_frame = Frame::new(bnds.x, bnds.y, bnds.w, bnds.h, "");

        let mut grp = Group::new(bnds.x, bnds.y, bnds.w, bnds.h, "");
        let mut rand_frame = Frame::new(bnds.x, bnds.y, bnds.w, bnds.h, "");
        grp.set_frame(app::frame_type());
        grp.set_color(Palette::BG0_H);
        draw_frame.set_frame(FrameType::FlatBox);
        draw_frame.set_color(Palette::BG0);

        let mut rng = rand::thread_rng();
        rand_frame.set_frame(FrameType::RoundedFrame);
        rand_frame.set_color(init_color);

        let mut button = Button::new(bnds.x, bnds.y, bnds.w, bnds.h, "").with_align(Align::Wrap);
        button.set_frame(FrameType::NoBox);
        button.set_down_frame(FrameType::NoBox);
        button.set_label_font(font);

        grp.end();

        const LERP_INC: f32 = 1. / 10.;
        const ITER_TIME: f64 = 1. / 144.;
        let fade_color = Palette::BG0_H;
        let mut rand_color = Color::Black;
        let mut fade_lerp = 0.;
        let mut continue_fading = false;
        grp.handle(move |g, ev| match ev {
            Event::Enter => {
                loop {
                    let new_rand_color = Palette::COLORS.into_iter().choose(&mut rng).unwrap();
                    if rand_color != new_rand_color {
                        rand_color = new_rand_color;
                        break;
                    }
                }
                draw::set_cursor(Cursor::Hand);
                if continue_fading {
                    continue_fading = false;
                }
                rand_frame.set_color(rand_color);
                g.set_color(Color::BackGround);
                draw_frame.redraw();
                rand_frame.redraw();
                g.redraw();
                true
            }
            Event::Leave => {
                draw::set_cursor(Cursor::Default);
                continue_fading = true;
                app::handle_main(fade).unwrap();
                true
            }
            _ if ev.bits() == fade => {
                if !continue_fading || fade_lerp > (1. + f32::EPSILON) {
                    fade_lerp = 0.;
                    return true;
                }

                let (cur_r, cur_g, cur_b) = g.color().to_rgb();
                let current_color = image::Color::new(cur_r, cur_g, cur_b, 255);

                let fade_color_int = fade_color.to_internal();
                let faded_color = current_color.lerp(fade_color_int, fade_lerp);
                fade_lerp += LERP_INC;
                g.set_color(Color::from_internal(faded_color));
                draw_frame.redraw();
                rand_frame.redraw();
                g.redraw();
                app::add_timeout3(ITER_TIME, move |_| {
                    let _ = app::handle_main(fade);
                });
                true
            }
            _ if ev.bits() == button_pushed => {
                g.set_color(rand_color.darker().lighter().darker().lighter().darker());
                draw_frame.redraw();
                rand_frame.redraw();
                g.redraw();
                true
            }
            _ if ev.bits() == button_released => {
                g.set_color(Color::BackGround);
                draw_frame.redraw();
                rand_frame.redraw();
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

        app::set_visible_focus(false);
        app::set_frame_type(FrameType::RFlatBox);
        app::set_frame_border_radius_max(10);
        app::add_handler(|ev| matches!(ev, Event::Shortcut) && (app::event_key() == (Key::Escape)));

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
        let (r, g, b) = Palette::FG2.to_rgb();
        app::set_foreground_color(r, g, b);

        let (r, g, b) = Palette::BG1.to_rgb();
        app::set_background_color(r, g, b);

        self.window.set_color(Color::BackGround);

        let (win_w, win_h) = (self.window.w(), self.window.h());

        self.window.make_resizable(true);
        self.window.size_range(800, 700, 3480, 2160);

        const GAP: i32 = 5;
        const MIDDLE_OFFSET: i32 = 50;

        // Sliders & crop widget (right side)
        let right_x = (win_w / 2) + MIDDLE_OFFSET;
        let right_y = GAP;
        let right_w = ((win_w - (GAP * 2)) / 2) - MIDDLE_OFFSET;
        let slider_h = (win_w as f32 * 0.05) as i32;

        // crop widget
        let crop_box_b = self.create_crop_widget(
            right_x,
            right_y,
            screen_aspect_ratio,
            slider_h,
            right_w,
            GAP,
        );

        // slider group
        let mut cur_slider_b = Bounds::new(
            right_x,
            crop_box_b.y + crop_box_b.h + GAP,
            right_w,
            slider_h,
        );
        let mut slider_grp_b = cur_slider_b;
        let mut colors_cycle = Palette::COLORS.into_iter().cycle().skip(2); // crop sliders took the first two colors

        CfgKey::iter()
            .filter(|key| !matches!(key, CfgKey::CropW | CfgKey::CropH))
            .filter(|key| matches!(key.default_val(), ValType::Unsigned(_) | ValType::Float(_)))
            .for_each(|key| {
                self.create_config_slider(
                    cur_slider_b,
                    key,
                    key.as_string(),
                    colors_cycle.next().unwrap(),
                );
                cur_slider_b.y += cur_slider_b.h + GAP;
            });
        slider_grp_b.h = cur_slider_b.y - slider_grp_b.y;

        // keycode button group
        let buttons_y = slider_grp_b.y + slider_grp_b.h;
        self.create_cfg_button_group(
            Bounds::new(right_x, buttons_y, right_w, (win_w - buttons_y) - GAP),
            3,
            cfg_path,
            GAP,
        );

        // Screen mirror widget, graph, and terminal (left side)
        let left_w = (win_w / 2) + MIDDLE_OFFSET;
        let left_h = win_h / 3;

        let frm_b = Bounds::new(0, 0, left_w, left_h + (GAP * 2)).gapify(GAP);
        let mut img_frame = Frame::new(frm_b.x, frm_b.y, frm_b.w, frm_b.h, "");
        let mut img_frame_img = image::zeroed::<Rgba8>(frm_b.w as usize, frm_b.h as usize);

        let mut graph = Graph::<5>::new(
            Bounds::new(0, frm_b.y + frm_b.h, left_w, left_h).gapify(GAP),
            5..50,
        );
        let mut term =
            Self::create_term(Bounds::new(0, graph.b.y + graph.b.h, left_w, left_h).gapify(GAP));
        let mut style_buffer = TextBuffer::default();
        let entries: Vec<StyleTableEntry> = vec![
            StyleTableEntry {
                // A
                color: Color::ForeGround,
                font: Font::Courier,
                size: 12,
            },
            StyleTableEntry {
                // B
                color: Palette::RED,
                font: Font::CourierBold,
                size: 12,
            },
        ];

        let mut now = Instant::now();
        app::add_idle3(move |_| {
            // blinking terminal cursor
            if now.elapsed() > Duration::from_secs_f32(0.5) {
                if term.cursor_color() == term.color() {
                    term.set_cursor_color(Color::ForeGround);
                    term.redraw();
                } else {
                    term.set_cursor_color(term.color());
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

                img_frame_img.fill_color(Palette::BG0.to_internal());
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
        term.set_selection_color(Color::ForeGround);
        term.set_color(Palette::BG0_H);
        term.set_cursor_color(Color::ForeGround);
        term.set_cursor_style(fltk::text::Cursor::Simple);
        term.set_scrollbar_size(-1); // no scrollbar
        term.set_ansi(true);
        term.set_frame(app::frame_type());
        term
    }

    fn create_cfg_button_group(&self, b: Bounds, row_len: i32, cfg_path: &'static str, gap: i32) {
        let pretty_name = |key: CfgKey| match key {
            CfgKey::AimKeycode => "Start Aim".to_string(),
            CfgKey::ToggleAimKeycode => "Toggle Aim".to_string(),
            CfgKey::AutoclickKeycode => "Autoclick".to_string(),
            CfgKey::ToggleAutoclickKeycode => "Cycle Autoclick Mode".to_string(),
            CfgKey::FakeLmbKeycode => "Fake Lmb".to_string(),
            _ => panic!("Keycode match not exhaustive"),
        };
        let mut bg_frame = Frame::new(b.x, b.y, b.w, b.h, "");
        bg_frame.set_color(Palette::BG0);
        bg_frame.set_frame(app::frame_type());

        let b = b.gapify(gap);

        let n_buttons = CfgKey::iter().filter(|k| k.is_keycode()).count() as i32;

        let button_w = b.w / row_len;
        let button_h = b.h / ((button_w * n_buttons) as f32 / b.w as f32).ceil() as i32;

        let mut colors_cycle = Palette::COLORS.into_iter().cycle();
        let mut current_bounds = Bounds::new(b.x, b.y, button_w, button_h);
        for key in CfgKey::iter().filter(|k| k.is_keycode()) {
            self.create_keycode_but(
                current_bounds.gapify(gap),
                key,
                pretty_name(key),
                colors_cycle.next().unwrap(),
            );
            current_bounds.x += button_w;
            if current_bounds.x + button_w > b.x + b.w {
                current_bounds.x = b.x;
                current_bounds.y += button_h;
            }
        }
        self.create_save_config_but(
            current_bounds.gapify(gap),
            cfg_path,
            colors_cycle.next().unwrap(),
        );
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
            Palette::COLORS[0],
        );
        let mut slider2 = self.create_config_slider(
            Bounds::new(x, slider2_ypos, box_w, slider_h),
            CfgKey::CropH,
            CfgKey::CropH.as_string(),
            Palette::COLORS[1],
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

    fn create_save_config_but(&self, b: Bounds, cfg_path: &'static str, c: Color) {
        let ResponsiveButton {
            b: _,
            mut button,
            push_event: button_pushed,
            release_event: button_released,
        } = ResponsiveButton::new(b, "Save config to file".to_string(), Font::CourierBold, c);

        button.set_label_size(12);
        button.draw(|b| {
            b.set_label_size(clamp(b.h() / 6, 1, 12));
        });

        let config = self.config.clone();
        button.handle(move |_, ev| match ev {
            Event::Push => {
                app::handle_main(button_pushed).unwrap();
                true
            }
            Event::Released => {
                app::handle_main(button_released).unwrap();
                let abs_cfg_path = match std::path::Path::new(cfg_path).canonicalize() {
                    Ok(abs_path) => abs_path.to_string_lossy().into_owned().split_off(4), // Removing windows extended path prefix
                    Err(_) => cfg_path.to_string(),
                };
                match config.write().unwrap().write_to_file(cfg_path) {
                    Ok(_) => {
                        log!("Saved config to {}", abs_cfg_path);
                    }
                    Err(e) => log_err!("Error saving config to {}:\n\t{}", abs_cfg_path, e),
                }
                true
            }
            _ => false,
        });
    }

    fn create_keycode_but(&self, b: Bounds, cfg_key: CfgKey, label: String, c: Color) -> Button {
        assert!(cfg_key.is_keycode());

        let capture_input = unique_event_id();

        let init_keycode: u16 = self.config.read().unwrap().get(cfg_key).into();
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
        } = ResponsiveButton::new(b, "".to_string(), Font::Courier, c);

        // Label frames
        const FONT_SIZE: i32 = 12;
        let labels_gap = (b.h as f32 * 0.35) as i32;
        let (center_x, center_y) = (b.x + (b.w / 2), b.y + (b.h / 2));
        let mut name_label =
            Frame::new(center_x, center_y - (labels_gap / 2), 0, 0, "").with_align(Align::Center);
        let val_label = Rc::new(RefCell::new(
            Frame::new(center_x, center_y + (labels_gap / 2), 0, 0, "").with_align(Align::Center),
        ));

        name_label.set_label_font(Font::Courier);
        name_label.set_label_size(FONT_SIZE);
        name_label.set_label_wrap(format!("{}:", label), button.width());
        val_label.borrow_mut().set_label_font(Font::CourierBold);
        val_label
            .borrow_mut()
            .set_label(&format!("'{}'", init_string));
        val_label.borrow_mut().set_label_size(FONT_SIZE);

        let val_label_clone = val_label.clone();
        button.draw(move |b| {
            val_label_clone
                .borrow_mut()
                .set_label_size(clamp(b.h() / 6, 1, FONT_SIZE));
            name_label.set_label_size(clamp(b.h() / 6, 1, FONT_SIZE));

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
                        Err(_) => {
                            val_label.borrow_mut().set_label(&last_label);
                            // but.redraw();
                            log_err!("Invalid keycode received: {}", keycode);
                        }
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

    fn create_config_slider(
        &self,
        b: Bounds,
        cfg_key: CfgKey,
        label: String,
        color: Color,
    ) -> HorFillSlider {
        let mut draw_frame = Frame::new(b.x, b.y, b.w, b.h, "");
        let mut slider = HorFillSlider::new(b.x, b.y, b.w, b.h, "");
        draw_frame.set_frame(FrameType::FlatBox);
        draw_frame.set_color(Color::BackGround);

        slider.set_color(Palette::BG0_H);
        slider.set_selection_color(color);
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
                (v.val as f64 * 100.).round() / 100.,
                (*v.bounds.start() as f64 * 100.).ceil() / 100.,
                (*v.bounds.end() as f64 * 100.).floor() / 100.,
                2,
            ),
            _ => panic!("Creating config slider from unbounded value"),
        };

        slider.set_precision(precision);
        slider.set_bounds(bounds_start, bounds_end); // -1 since bounds are inclusive
        slider.set_value(cfg_val);

        const LABEL_SIZE_SCALAR: f32 = 0.3;
        let mut label_frame = Frame::new(b.x, b.y, b.w, b.h, "")
            .with_label(format!("{}: {}", label, cfg_val).as_str());
        label_frame.set_label_font(Font::Courier);

        slider.draw(move |slider| {
            label_frame.set_label(format!("{}: {}", label, slider.value()).as_str());
            label_frame.redraw_label();
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
    let (x, y, w, h) = Coord::bbox_xywh(&coord_cluster[..]);
    let img_center = Coord::new(img.w / 2, img.h / 2);
    img.draw_bbox(Coord::new(x, y), w, h, Palette::GREEN.to_internal());
    img.draw_crosshair(img_center, 10, Palette::YELLOW.to_internal());
    if img_center.square_dist(aim_coord) > 4 {
        img.draw_crosshair(aim_coord, 10, Palette::RED.to_internal());
        img.draw_line(img_center, aim_coord, Palette::AQUA.to_internal());
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
