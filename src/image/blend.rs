#![allow(overflowing_literals)]

use crate::image::Color;
use std::arch::x86_64::*;

pub enum BlendType {
    Over,
    Under,
}

// https://en.wikipedia.org/wiki/Alpha_compositing#Description
pub fn over(fg: Color<u8>, bg: Color<u8>) -> Color<u8> {
    let alpha = fg.a as u16 + 1;
    let inv_alpha = 256 - fg.a as u16;

    Color::new(
        ((alpha * fg.r as u16 + inv_alpha * bg.r as u16) >> 8) as u8,
        ((alpha * fg.g as u16 + inv_alpha * bg.g as u16) >> 8) as u8,
        ((alpha * fg.b as u16 + inv_alpha * bg.b as u16) >> 8) as u8,
        255,
    )
}

pub fn under(fg_px: Color<u8>, bg_px: Color<u8>) -> Color<u8> {
    over(bg_px, fg_px)
}

#[target_feature(enable = "avx2")]
pub unsafe fn avx_blend_over(pixels_fg: *const u8, pixels_bg: *const u8, dst: *mut u8) {
    // alpha indicies for each subpixel
    let alpha_shuffle = _mm256_setr_epi8(
        3, 3, 3, 3, 7, 7, 7, 7, 11, 11, 11, 11, 15, 15, 15, 15, 19, 19, 19, 19, 23, 23, 23, 23, 27,
        27, 27, 27, 31, 31, 31, 31,
    );

    // [r|g|b|a ... ]
    let fg = _mm256_loadu_si256(pixels_fg as *const _);
    let bg = _mm256_loadu_si256(pixels_bg as *const _);

    // convert to signed range for maddubs
    let fg_signed = _mm256_add_epi8(fg, _mm256_set1_epi8(i8::MIN));
    let bg_signed = _mm256_add_epi8(bg, _mm256_set1_epi8(i8::MIN));

    // [a1|a1|a1|a1|a2|a2|a2|a2 ... ]
    let alpha = _mm256_shuffle_epi8(fg, alpha_shuffle);
    let alpha_inv = _mm256_subs_epu8(_mm256_set1_epi8(-1 /*255*/), alpha);

    // [a1|inv1|a2|inv2 ... ]
    let a_ainv_lo = _mm256_unpacklo_epi8(alpha, alpha_inv);
    let a_ainv_hi = _mm256_unpackhi_epi8(alpha, alpha_inv);

    // [fgr|bgr|fgg|bgg ... ]
    let fg_bg_lo = _mm256_unpacklo_epi8(fg_signed, bg_signed);
    let fg_bg_hi = _mm256_unpackhi_epi8(fg_signed, bg_signed);

    // [(a1*fgr) + (inv1*bgr)|(a2*fgg) + (inv2*bgg) ... ]
    let mut ret_lo = _mm256_maddubs_epi16(a_ainv_lo, fg_bg_lo);
    let mut ret_hi = _mm256_maddubs_epi16(a_ainv_hi, fg_bg_hi);

    // back to unsigned range
    ret_lo = _mm256_add_epi16(ret_lo, _mm256_set1_epi16(i16::MAX));
    ret_hi = _mm256_add_epi16(ret_hi, _mm256_set1_epi16(i16::MAX));

    // divide by 255
    ret_lo = _mm256_srli_epi16(_mm256_mulhi_epu16(ret_lo, _mm256_set1_epi16(0x8081)), 7);
    ret_hi = _mm256_srli_epi16(_mm256_mulhi_epu16(ret_hi, _mm256_set1_epi16(0x8081)), 7);

    // repack and store
    let ret = _mm256_packus_epi16(ret_lo, ret_hi);
    _mm256_storeu_si256(dst as *mut _, ret);
}

pub unsafe fn avx_blend_under(pixels_fg: *const u8, pixels_bg: *const u8, dst: *mut u8) {
    avx_blend_over(pixels_bg, pixels_fg, dst);
}
