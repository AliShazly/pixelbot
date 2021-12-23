extern crate line_drawing;
use crate::coord::Coord;
use crate::image::blend::{avx_blend_over, avx_blend_under, over, under};
use crate::image::{get_1d_idx, get_2d_idx, pack_rgb, Color, Image, Pixel, PixelMut, Subpixel};

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
        let row_range = crop_w_subpx..(w_subpx - crop_w_subpx);
        let col_range = crop_h..(self.h - crop_h);

        let mut out_buf: Vec<S::Inner> = Vec::new();
        self.rows()
            .enumerate()
            .filter_map(|(idx, row)| {
                if col_range.contains(&idx) {
                    Some(row.index(row_range.clone()))
                } else {
                    None
                }
            })
            .for_each(|slice| out_buf.extend_from_slice(slice));

        Image::new(out_buf, self.w - (2 * crop_w), self.h - (2 * crop_h))
    }
}

impl<T, S> Image<T, S>
where
    T: DerefMut<Target = [S::Inner]>,
    S: Subpixel,
{
    pub fn draw_line(&mut self, start: Coord<i32>, end: Coord<i32>, fill: Color<S::Inner>) {
        for (x, y) in line_drawing::WalkGrid::new((start.x, start.y), (end.x, end.y)) {
            let idx = get_1d_idx(self.w, x as usize, y as usize);
            self.get_pixel_mut(idx).set(fill);
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

    pub fn color_range_avg_pos(
        &self,
        target: Color<S::Inner>,
        thresh: f32,
        y_divisor: f32, // Divides Y to bias aim towards head
    ) -> Option<Coord<usize>> {
        assert!(thresh > 0. && thresh < 1.);

        let mut count: u32 = 0;
        let mut coord_sum = Coord::new(0, 0);

        self.pixels()
            .map(|px| 1. - color_distance(px.as_color(), target))
            .enumerate()
            .for_each(|(idx, dist)| {
                if dist > thresh {
                    coord_sum += get_2d_idx(self.w, idx);
                    count += 1;
                }
            });

        const ALLOWED_NOISE: u32 = 50;
        if count > ALLOWED_NOISE {
            Some(Coord::new(
                coord_sum.x / count as usize,
                ((coord_sum.y / count as usize) as f32 / y_divisor) as usize,
            ))
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
