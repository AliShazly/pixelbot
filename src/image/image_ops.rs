use crate::coord::Coord;
use crate::image::{get_1d_idx, get_2d_idx, Image, Pixel, Subpx, SubpxOrder, N_SUBPX};

use std::assert;
use std::ops::{Deref, DerefMut, Index};

pub mod blend_fns {
    use crate::image::{Pixel, Subpx};

    // https://en.wikipedia.org/wiki/Alpha_compositing#Description
    pub fn over(fg_px: Pixel, bg_px: Pixel) -> Pixel {
        let (alpha_fg, alpha_bg) = (
            fg_px.a as f32 / Subpx::MAX as f32,
            bg_px.a as f32 / Subpx::MAX as f32,
        );
        let (fg_r, fg_g, fg_b) = (fg_px.r as f32, fg_px.g as f32, fg_px.b as f32);
        let (bg_r, bg_g, bg_b) = (bg_px.r as f32, bg_px.g as f32, bg_px.b as f32);
        let alpha_fg_inv = 1. - alpha_fg;

        let a_out = alpha_fg + (alpha_bg * alpha_fg_inv);
        Pixel::new(
            (((fg_r * alpha_fg) + ((bg_r * alpha_bg) * alpha_fg_inv)) / a_out) as u8,
            (((fg_g * alpha_fg) + ((bg_g * alpha_bg) * alpha_fg_inv)) / a_out) as u8,
            (((fg_b * alpha_fg) + ((bg_b * alpha_bg) * alpha_fg_inv)) / a_out) as u8,
            (a_out * Subpx::MAX as f32) as u8,
        )
    }

    pub fn under(fg_px: Pixel, bg_px: Pixel) -> Pixel {
        over(bg_px, fg_px) // it really feels good to be a genius
    }
}

impl<T: Deref<Target = [Subpx]>> Image<T> {
    pub fn crop_to_center(&self, crop_w: usize, crop_h: usize) -> Image<Vec<u8>> {
        assert!(
            crop_w * 2 < self.w && crop_h * 2 < self.h,
            "Cropping out of bounds"
        );

        let w_subpx = self.w * N_SUBPX;
        let crop_w_subpx = crop_w * N_SUBPX;
        let row_range = crop_w_subpx..(w_subpx - crop_w_subpx);
        let col_range = crop_h..(self.h - crop_h);

        let mut out_buf: Vec<u8> = Vec::new();
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

        Image::new(
            out_buf,
            SubpxOrder::BGRA,
            self.w - (2 * crop_w),
            self.h - (2 * crop_h),
        )
    }

    pub fn color_range_avg_pos(
        &self,
        target: Pixel,
        thresh: f32,
        y_divisor: f32, // Divides Y to bias aim towards head
    ) -> Option<Coord<usize>> {
        assert!(thresh > 0. && thresh < 1.);

        let mut count: u32 = 0;
        let mut coord_sum = Coord::new(0, 0);

        self.pixels()
            .map(|px| 1. - color_distance(px, target))
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
}

impl<T: DerefMut<Target = [Subpx]>> Image<T> {
    pub fn draw_line(&mut self, start: Coord<i32>, end: Coord<i32>, fill: Pixel) {
        let order = self.pixel_order;
        for (x, y) in line_drawing::WalkGrid::new((start.x, start.y), (end.x, end.y)) {
            let idx = get_1d_idx(self.w, x as usize, y as usize);
            self.get_pixel_mut(idx).set(fill, order);
        }
    }

    pub fn draw_grid(&mut self, step: u32, fill: Pixel) {
        let img_w = self.w;
        let pixel_order = self.pixel_order;

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
                px.set(fill, pixel_order);
                col_draw = 0;
            } else {
                col_draw += 1;
            }

            if row_draw == step {
                px.set(fill, pixel_order);
            }

            cur_col += 1;
        });
    }

    pub fn blend(
        &mut self,
        blend_fn: impl Fn(Pixel, Pixel) -> Pixel,
        other_img: &Image<impl Deref<Target = [Subpx]>>,
    ) {
        assert!(self.w == other_img.w && self.h == other_img.h);

        let order = self.pixel_order;
        self.pixels_mut()
            .zip(other_img.pixels())
            .for_each(|(mut fg_px, bg_px)| {
                let out_px = blend_fn(fg_px.as_pixel(order), bg_px);
                fg_px.set(out_px, order);
            });
    }
}

// https://www.compuphase.com/cmetric.htm
fn color_distance(p1: Pixel, p2: Pixel) -> f32 {
    let rmean = (p1.r as i32 + p2.r as i32) / 2;
    let r = p1.r as i32 - p2.r as i32;
    let g = p1.g as i32 - p2.g as i32;
    let b = p1.b as i32 - p2.b as i32;
    f32::sqrt(((((512 + rmean) * r * r) >> 8) + 4 * g * g + (((767 - rmean) * b * b) >> 8)) as f32)
        / (255 * 3) as f32
}
