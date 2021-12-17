extern crate line_drawing;
use crate::coord::Coord;
use std::assert;
use std::ops::{Deref, DerefMut, Sub};

pub mod image_ops;

pub type Subpx = u8;
pub const N_SUBPX: usize = 4;
type SubpxIdxs = (usize, usize, usize, usize);
const RGBA_ORDER: SubpxIdxs = (0, 1, 2, 3);
const BGRA_ORDER: SubpxIdxs = (2, 1, 0, 3);

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy)]
pub enum SubpxOrder {
    RGBA,
    BGRA,
}

impl SubpxOrder {
    fn idxs(&self) -> SubpxIdxs {
        match *self {
            Self::RGBA => RGBA_ORDER,
            Self::BGRA => BGRA_ORDER,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Pixel {
    pub r: Subpx,
    pub g: Subpx,
    pub b: Subpx,
    pub a: Subpx,
}

impl Pixel {
    pub fn new(r: Subpx, g: Subpx, b: Subpx, a: Subpx) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_slice(slice: &[Subpx], pixel_order: SubpxIdxs) -> Self {
        assert!(slice.len() >= N_SUBPX);

        let (r_idx, g_idx, b_idx, a_idx) = pixel_order;
        Self::new(slice[r_idx], slice[g_idx], slice[b_idx], slice[a_idx])
    }

    pub fn packed(&self) -> u32 {
        let (r, g, b) = (self.r as u32, self.g as u32, self.b as u32);
        (r << 16) | (g << 8) | b
    }
}

pub struct PixelMut<'a>(&'a mut [Subpx]);

impl<'a> Deref for PixelMut<'a> {
    type Target = [Subpx];
    fn deref(&self) -> &Self::Target {
        self.0
    }
}
impl<'a> DerefMut for PixelMut<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl<'a> PixelMut<'a> {
    pub fn set(&mut self, px: Pixel, self_order: SubpxIdxs) {
        let (r_idx, g_idx, b_idx, a_idx) = self_order;
        self.0[r_idx] = px.r;
        self.0[g_idx] = px.g;
        self.0[b_idx] = px.b;
        self.0[a_idx] = px.a;
    }
    pub fn set_a(&mut self, alpha: Subpx) {
        self.0[N_SUBPX - 1] = alpha;
    }
    pub fn as_pixel(&self, order: SubpxIdxs) -> Pixel {
        let (r_idx, g_idx, b_idx, a_idx) = order;
        Pixel::new(self.0[r_idx], self.0[g_idx], self.0[b_idx], self.0[a_idx])
    }
}

#[derive(Debug)]
pub struct Image<T>
where
    T: Deref<Target = [Subpx]>,
{
    buf: T,
    pixel_order: SubpxIdxs,
    pub w: usize,
    pub h: usize,
}

impl<T: Deref<Target = [Subpx]>> Deref for Image<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl<T: DerefMut<Target = [Subpx]>> DerefMut for Image<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buf
    }
}

impl<T: Deref<Target = [Subpx]>> Image<T> {
    pub fn new(buf: T, pixel_order: SubpxOrder, w: usize, h: usize) -> Self {
        assert!(
            buf.len() == (w * h * N_SUBPX),
            "Image dims don't match buffer length"
        );
        Self {
            buf,
            pixel_order: pixel_order.idxs(),
            w,
            h,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Subpx> {
        self.buf.iter()
    }

    pub fn pixels(&self) -> impl Iterator<Item = Pixel> + '_ {
        self.buf
            .chunks_exact(N_SUBPX)
            .map(|slice| Pixel::from_slice(slice, self.pixel_order))
    }

    pub fn rows(&self) -> impl Iterator<Item = &[Subpx]> {
        self.buf.chunks_exact(self.w * N_SUBPX)
    }

    // Takes pixel idx.
    //     eg. idx=1 -> 2nd pixel, not 2nd subpx
    pub fn get_pixel(&self, pixel_idx: usize) -> Pixel {
        let buf_idx: usize = pixel_idx * N_SUBPX;
        Pixel::from_slice(&self.buf[buf_idx..buf_idx + N_SUBPX], self.pixel_order)
    }

    pub fn show(&self) {
        let (w, h) = (self.w, self.h);
        let buf_packed: Vec<u32> = (0..self.w * self.h)
            .map(|idx| self.get_pixel(idx).packed())
            .collect();
        let mut window =
            minifb::Window::new("Image", w, h, minifb::WindowOptions::default()).unwrap();
        window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));
        while window.is_open() && !window.is_key_down(minifb::Key::Escape) {
            // window.set_position(-1920, 0);
            window.update_with_buffer(&buf_packed, w, h).unwrap();
        }
    }
}

impl<T: DerefMut<Target = [Subpx]>> Image<T> {
    pub fn pixels_mut(&mut self) -> impl Iterator<Item = PixelMut> + '_ {
        self.buf
            .chunks_exact_mut(N_SUBPX)
            .map(|slice| PixelMut(slice))
    }

    pub fn get_pixel_mut(&mut self, pixel_idx: usize) -> PixelMut {
        let buf_idx: usize = pixel_idx * N_SUBPX;
        PixelMut(&mut self.buf[buf_idx..buf_idx + N_SUBPX])
    }

    pub fn fill_zeroes(&mut self) {
        self.buf.fill(0);
    }

    pub fn fill_alpha(&mut self, alpha: Subpx) {
        self.pixels_mut().for_each(|mut px| {
            px.set_a(alpha);
        });
    }

    pub fn fill_color(&mut self, color: Pixel) {
        let order = self.pixel_order;
        self.pixels_mut().for_each(|mut px| {
            px.set(color, order);
        });
    }
}

fn get_2d_idx(width: usize, idx: usize) -> Coord<usize> {
    let x = idx % width;
    let y = idx / width;
    Coord::new(x, y)
}

fn get_1d_idx(width: usize, row: usize, col: usize) -> usize {
    col + (row * width)
}
