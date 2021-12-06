use crate::coord::Coord;
use minifb;
use std::assert;
use std::ops::Deref;

pub const N_SUBPX: usize = 4;
pub type Subpx = u8;

#[derive(Debug, Clone, Copy)]
pub enum PixelOrder {
    RGBA,
    BGRA,
}

impl PixelOrder {
    pub fn rgb_ordering(&self) -> (usize, usize, usize) {
        match *self {
            Self::RGBA => (0, 1, 2),
            Self::BGRA => (2, 1, 0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Pixel {
    pub r: Subpx,
    pub g: Subpx,
    pub b: Subpx,
}

impl Pixel {
    pub fn new(r: Subpx, g: Subpx, b: Subpx) -> Self {
        Self { r, g, b }
    }

    pub fn from_slice(slice: &[Subpx], pixel_order: PixelOrder) -> Self {
        assert!(
            slice.len() >= 3,
            "Cannot create pixel with less than 3 elements"
        );

        let (r_idx, g_idx, b_idx) = pixel_order.rgb_ordering();
        Self::new(slice[r_idx], slice[g_idx], slice[b_idx])
    }

    pub fn packed(&self) -> u32 {
        let (r, g, b) = (self.r as u32, self.g as u32, self.b as u32);
        (r << 16) | (g << 8) | b
    }
}

#[derive(Debug)]
pub struct Image<T: Deref<Target = [Subpx]>> {
    buf: T,
    pixel_order: PixelOrder,
    pub w: usize,
    pub h: usize,
}

impl<T: Deref<Target = [Subpx]>> Image<T> {
    pub fn new(buf: T, pixel_order: PixelOrder, w: usize, h: usize) -> Self {
        assert!(
            buf.len() == (w * h * N_SUBPX),
            "Image dims don't match buffer length"
        );
        Self {
            buf,
            pixel_order,
            w,
            h,
        }
    }

    pub fn get_2d_idx(&self, idx: usize) -> Coord<usize> {
        let x = idx % self.w;
        let y = idx / self.w;
        Coord::new(x, y)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Subpx> {
        self.buf.iter()
    }

    pub fn pixels<'a>(&'a self) -> impl Iterator<Item = Pixel> + 'a {
        self.buf
            .chunks_exact(N_SUBPX)
            .map(|slice| Pixel::from_slice(slice, self.pixel_order))
    }

    pub fn rows(&self) -> impl Iterator<Item = &[Subpx]> {
        self.buf.chunks_exact(self.w * N_SUBPX)
    }

    pub fn get_pixel(&self, idx: usize) -> Pixel {
        let buf_idx: usize = idx * N_SUBPX;
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
            window.set_position(-1920, 0);
            window.update_with_buffer(&buf_packed, w, h).unwrap();
            std::thread::sleep(std::time::Duration::from_secs_f32(0.5));
            break;
        }
    }
}
