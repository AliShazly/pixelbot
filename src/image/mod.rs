use num::Zero;

use crate::coord::Coord;
use std::assert;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

mod blend;
pub mod image_ops;

pub struct SubpxOrder {
    r: usize,
    g: usize,
    b: usize,
    a: usize,
}
const RGBA_ORDER: SubpxOrder = SubpxOrder { r: 0, g: 1, b: 2, a: 3 };
const BGRA_ORDER: SubpxOrder = SubpxOrder { r: 2, g: 1, b: 0, a: 3 };

#[derive(Debug, Clone, Copy)]
pub struct Color<T> {
    pub r: T,
    pub g: T,
    pub b: T,
    pub a: T,
}
impl<T: Copy> Color<T> {
    pub fn new(r: T, g: T, b: T, a: T) -> Self {
        Self { r, g, b, a }
    }
}

pub trait Subpixel {
    type Inner: Copy + Zero;

    const ORDER: SubpxOrder;
    const N_SUBPX: usize;
}

pub trait Pixel<T: Subpixel>
where
    Self: AsRef<[T::Inner]>,
{
    fn rgba(&self) -> [T::Inner; 4] {
        [
            self.as_ref()[T::ORDER.r],
            self.as_ref()[T::ORDER.g],
            self.as_ref()[T::ORDER.b],
            self.as_ref()[T::ORDER.a],
        ]
    }
    fn as_color(&self) -> Color<T::Inner> {
        let [r, g, b, a] = self.rgba();
        Color::new(r, g, b, a)
    }
}

pub trait PixelMut<T: Subpixel>: Pixel<T> {
    fn set(&mut self, fill: Color<T::Inner>);
    fn set_a(&mut self, alpha: T::Inner);
}

impl<T: Subpixel> Pixel<T> for &[T::Inner] {}
impl<T: Subpixel> Pixel<T> for &mut [T::Inner] {}
impl<T: Subpixel> PixelMut<T> for &mut [T::Inner] {
    fn set(&mut self, fill: Color<T::Inner>) {
        self[T::ORDER.r] = fill.r;
        self[T::ORDER.g] = fill.g;
        self[T::ORDER.b] = fill.b;
        self[T::ORDER.a] = fill.a;
    }
    fn set_a(&mut self, alpha: T::Inner) {
        self[T::ORDER.a] = alpha;
    }
}

macro_rules! define_subpx {
    ($name:ident, $typ:ty, $order: expr, $n_subpx: expr) => {
        pub enum $name {}

        impl Subpixel for $name {
            type Inner = $typ;
            const ORDER: SubpxOrder = $order;
            const N_SUBPX: usize = $n_subpx;
        }
    };
}

define_subpx!(Rgba8, u8, RGBA_ORDER, 4);
define_subpx!(Bgra8, u8, BGRA_ORDER, 4);

#[derive(Debug)]
pub struct Image<T, S> {
    buf: T,
    pub w: usize,
    pub h: usize,
    _marker: PhantomData<fn() -> S>,
}

impl<T, S> Image<T, S>
where
    T: Deref<Target = [S::Inner]>,
    S: Subpixel,
{
    pub fn new(buf: T, w: usize, h: usize) -> Self {
        assert!(
            buf.len() == (w * h * S::N_SUBPX),
            "Image dims don't match buffer length"
        );
        Self {
            buf,
            w,
            h,
            _marker: PhantomData,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &S::Inner> {
        self.buf.iter()
    }

    pub fn pixels(&self) -> impl Iterator<Item = impl Pixel<S> + '_> {
        self.buf.chunks_exact(S::N_SUBPX)
    }

    // maybe double split into chunks of pixels
    pub fn rows(&self) -> impl Iterator<Item = &[S::Inner]> {
        self.buf.chunks_exact(self.w * S::N_SUBPX)
    }

    // Takes pixel idx.
    //     eg. idx=1 -> 2nd pixel, not 2nd subpx
    pub fn get_pixel(&self, pixel_idx: usize) -> impl Pixel<S> + '_ {
        let buf_idx: usize = pixel_idx * S::N_SUBPX;
        &self.buf[buf_idx..buf_idx + S::N_SUBPX]
    }

    pub fn as_slice(&self) -> &[S::Inner] {
        &self.buf[..]
    }
}

impl<T, S> Image<T, S>
where
    T: DerefMut<Target = [S::Inner]>,
    S: Subpixel,
{
    pub fn pixels_mut(&mut self) -> impl Iterator<Item = impl PixelMut<S> + '_> {
        self.buf.chunks_exact_mut(S::N_SUBPX)
    }

    pub fn get_pixel_mut(&mut self, pixel_idx: usize) -> impl PixelMut<S> + '_ {
        let buf_idx: usize = pixel_idx * S::N_SUBPX;
        &mut self.buf[buf_idx..buf_idx + S::N_SUBPX]
    }

    pub fn fill_zeroes(&mut self) {
        self.buf.fill(num::zero());
    }

    pub fn fill_color(&mut self, color: Color<S::Inner>) {
        self.pixels_mut().for_each(|mut px| {
            px.set(color);
        });
    }
}

pub fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    let (r, g, b) = (r as u32, g as u32, b as u32);
    (r << 16) | (g << 8) | b
}

fn get_2d_idx(width: usize, idx: usize) -> Coord<usize> {
    let x = idx % width;
    let y = idx / width;
    Coord::new(x, y)
}

fn get_1d_idx(width: usize, row: usize, col: usize) -> usize {
    col + (row * width)
}
