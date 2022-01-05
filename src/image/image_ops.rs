extern crate line_drawing;
use crate::coord::Coord;
use crate::image::blend::{avx_blend_over, avx_blend_under, over, under};
use crate::image::{get_2d_idx, pack_rgb, Color, Image, Pixel, PixelMut, Subpixel};

use std::assert;
use std::ops::{Deref, DerefMut, Index};

pub use crate::image::blend::BlendType;

impl<T, S> Image<T, S>
where
    T: Deref<Target = [S::Inner]>,
    S: Subpixel,
{
    pub fn crop_to_center(&self, crop_w: usize, crop_h: usize) -> Image<Vec<S::Inner>, S> {
        assert!(
            crop_w * 2 < self.w && crop_h * 2 < self.h,
            "Cropping out of bounds"
        );

        let w_subpx = self.w * S::N_SUBPX;
        let crop_w_subpx = crop_w * S::N_SUBPX;
        let col_range = crop_w_subpx..(w_subpx - crop_w_subpx);
        let row_range = crop_h..(self.h - crop_h);

        let mut out_buf: Vec<S::Inner> = Vec::new();
        self.rows()
            .enumerate()
            .filter_map(|(idx, row)| {
                if row_range.contains(&idx) {
                    Some(row.index(col_range.clone()))
                } else {
                    None
                }
            })
            .for_each(|slice| out_buf.extend_from_slice(slice));

        Image::new(out_buf, self.w - (2 * crop_w), self.h - (2 * crop_h))
    }

    pub fn scale_nearest(&self, new_w: usize, new_h: usize) -> Image<Vec<S::Inner>, S> {
        assert!(new_w > 0 && new_h > 0);

        let mut out = Image::<Vec<_>, _>::zeroed(new_w, new_h);
        for x in 0..new_w {
            for y in 0..new_h {
                let src_x =
                    (self.w - 1).min((x as f32 / new_w as f32 * self.w as f32).round() as usize);
                let src_y =
                    (self.h - 1).min((y as f32 / new_h as f32 * self.h as f32).round() as usize);

                out.set2d(
                    Coord::new(x, y),
                    self.get_pixel2d(Coord::new(src_x, src_y)).as_color(),
                );
            }
        }
        out
    }

    pub fn scale_keep_aspect(&self, new_w: usize, new_h: usize) -> Image<Vec<S::Inner>, S> {
        let ratio = (new_w as f32 / self.w as f32).min(new_h as f32 / self.h as f32);
        self.scale_nearest(
            ((self.w as f32 * ratio) as usize).max(1),
            ((self.h as f32 * ratio) as usize).max(1),
        )
    }
}

impl<T, S> Image<T, S>
where
    T: DerefMut<Target = [S::Inner]>,
    S: Subpixel,
{
    pub fn draw_line(&mut self, start: Coord<usize>, end: Coord<usize>, fill: Color<S::Inner>) {
        line_drawing::Bresenham::new(
            (start.x as i32, start.y as i32),
            (end.x as i32, end.y as i32),
        )
        .for_each(|(x, y)| self.set2d(Coord::new(x as usize, y as usize), fill))
    }

    pub fn draw_bbox(&mut self, tl: Coord<usize>, w: usize, h: usize, fill: Color<S::Inner>) {
        let tr = Coord::new(tl.x + w, tl.y);
        let bl = Coord::new(tl.x, tl.y + h);
        let br = Coord::new(bl.x + w, bl.y);
        self.draw_line(tl, tr, fill);
        self.draw_line(tr, br, fill);
        self.draw_line(br, bl, fill);
        self.draw_line(bl, tl, fill);
    }

    pub fn draw_crosshair(&mut self, pos: Coord<usize>, len: usize, fill: Color<S::Inner>) {
        assert!((0..self.w).contains(&pos.x) && (0..self.h).contains(&pos.y));

        let x_range =
            (pos.x as i32 - len as i32).max(0) as usize..=(pos.x + len).min(self.w - 1) as usize;
        let y_range =
            (pos.y as i32 - len as i32).max(0) as usize..=(pos.y + len).min(self.h - 1) as usize;

        for x_idx in x_range {
            self.set2d(Coord::new(x_idx, pos.y), fill);
        }
        for y_idx in y_range {
            self.set2d(Coord::new(pos.x, y_idx), fill);
        }
    }

    pub fn draw_grid(&mut self, step: u32, fill: Color<S::Inner>) {
        let img_w = self.w;

        // ugly but 10x faster than the modulo version
        let mut cur_col: u32 = 0;
        let mut row_draw: u32 = 0;
        let mut col_draw: u32 = 0;
        self.pixels_mut().for_each(|mut px| {
            if cur_col == img_w as u32 {
                cur_col = 0;
                col_draw = 0;

                if row_draw == step {
                    row_draw = 0;
                } else {
                    row_draw += 1;
                }
            }

            if col_draw == step {
                px.set(fill);
                col_draw = 0;
            } else {
                col_draw += 1;
            }

            if row_draw == step {
                px.set(fill);
            }

            cur_col += 1;
        });
    }

    pub fn layer_image_over<U, V>(&mut self, other_img: &Image<U, V>)
    where
        U: DerefMut<Target = [V::Inner]>,
        V: Subpixel<Inner = S::Inner>,
    {
        assert!(other_img.w <= self.w && other_img.h <= self.h && V::N_SUBPX == S::N_SUBPX,);

        let col_skip = (self.w - other_img.w) / 2;
        let y_center_start = (self.h - other_img.h) / 2;
        let row_range = y_center_start..y_center_start + other_img.h;

        let mut other_img_rows = other_img.rows();
        self.rows_mut().enumerate().for_each(|(idx, row)| {
            if row_range.contains(&idx) {
                let mut other_row = other_img_rows.next().unwrap().chunks_exact(V::N_SUBPX);
                for mut px_slice in row
                    .chunks_exact_mut(S::N_SUBPX)
                    .skip(col_skip)
                    .take(other_img.w)
                {
                    let other_color = Pixel::<V>::as_color(&other_row.next().unwrap());
                    PixelMut::<S>::set(&mut px_slice, other_color);
                }
            }
        });
    }
}

impl<T, S> Image<T, S>
where
    T: DerefMut<Target = [S::Inner]>,
    S: Subpixel<Inner = u8>,
{
    pub fn blend(&mut self, blend_type: BlendType, other_img: &Image<T, S>) {
        assert!(self.w == other_img.w && self.h == other_img.h);

        if std::is_x86_feature_detected!("avx2") {
            let blend_fn = match blend_type {
                BlendType::Over => avx_blend_over,
                BlendType::Under => avx_blend_under,
            };

            const STEP: usize = 32; // 32 subpixels (8 RGBA pixels) at a time; 8 * S::N_SUBPX
            for idx in (0..self.w * self.h * S::N_SUBPX).step_by(STEP) {
                unsafe {
                    blend_fn(
                        self.buf[idx..].as_ptr(),
                        other_img.buf[idx..].as_ptr(),
                        self.buf[idx..].as_mut_ptr(), // I think this is okay since the first pointer is never used after the inital load
                    );
                }
            }
        } else {
            let blend_fn = match blend_type {
                BlendType::Over => over,
                BlendType::Under => under,
            };
            self.pixels_mut()
                .zip(other_img.pixels())
                .for_each(|(mut fg_px, bg_px)| {
                    let out_px = blend_fn(fg_px.as_color(), bg_px.as_color());
                    fg_px.set(out_px);
                });
        }
    }

    pub fn detect_color(&self, target: Color<S::Inner>, thresh: f32) -> Option<Vec<Coord<usize>>> {
        assert!(thresh > 0. && thresh < 1.);

        let mut coords: Vec<Coord<usize>> = Vec::new();
        self.pixels()
            .map(|px| 1. - color_distance(px.as_color(), target))
            .enumerate()
            .for_each(|(idx, dist)| {
                if dist > thresh {
                    coords.push(get_2d_idx(self.w, idx));
                }
            });

        const MIN_PIXELS: usize = 50;
        if coords.len() > MIN_PIXELS {
            Some(coords)
        } else {
            None
        }
    }

    pub fn show(&self) {
        let (width, height) = (self.w, self.h);
        let buf_packed: Vec<u32> = (0..self.w * self.h)
            .map(|idx| self.get_pixel(idx))
            .map(|px| {
                let [r, g, b, _] = px.rgba();
                pack_rgb(r, g, b)
            })
            .collect();
        let mut window =
            minifb::Window::new("Image", width, height, minifb::WindowOptions::default()).unwrap();
        window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));
        while window.is_open() && !window.is_key_down(minifb::Key::Escape) {
            // window.set_position(-1920, 0);
            window
                .update_with_buffer(&buf_packed, width, height)
                .unwrap();
        }
    }
}
// https://www.compuphase.com/cmetric.htm
fn color_distance(p1: Color<u8>, p2: Color<u8>) -> f32 {
    let rmean = (p1.r as i32 + p2.r as i32) / 2;
    let r = p1.r as i32 - p2.r as i32;
    let g = p1.g as i32 - p2.g as i32;
    let b = p1.b as i32 - p2.b as i32;
    f32::sqrt(((((512 + rmean) * r * r) >> 8) + 4 * g * g + (((767 - rmean) * b * b) >> 8)) as f32)
        / (255 * 3) as f32
}
