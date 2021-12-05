use crate::coord::Coord;
use crate::image::{Image, Pixel, PixelOrder, Subpx, N_SUBPX};

use std::assert;
use std::ops::{Deref, Index};

pub fn crop_to_center(
    img: &Image<impl Deref<Target = [Subpx]>>,
    crop_w: usize,
    crop_h: usize,
) -> Image<Vec<u8>> {
    assert!(
        crop_w * 2 < img.w && crop_h * 2 < img.h,
        "Cropping out of bounds"
    );

    let w_subpx = img.w * N_SUBPX;
    let crop_w_subpx = crop_w * N_SUBPX;
    let row_range = crop_w_subpx..(w_subpx - crop_w_subpx);
    let col_range = crop_h..(img.h - crop_h);

    let mut out_buf: Vec<u8> = Vec::new();
    img.rows()
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
        PixelOrder::BGRA,
        img.w - (2 * crop_w),
        img.h - (2 * crop_h),
    )
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

pub fn color_range_avg_pos(
    img: &Image<impl Deref<Target = [Subpx]>>,
    target: Pixel,
    thresh: f32,
    y_divisor: f32, // Divides Y to bias aim towards head
) -> Option<Coord<usize>> {
    assert!(thresh > 0. && thresh < 1.);

    let mut count: u32 = 0;
    let mut coord_sum = Coord::new(0, 0);

    img.pixels()
        .map(|px| 1. - color_distance(px, target))
        .enumerate()
        .for_each(|(idx, dist)| {
            if dist > thresh {
                coord_sum += img.get_2d_idx(idx);
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
