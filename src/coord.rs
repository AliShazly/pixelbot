use std::ops::{Add, AddAssign, Sub, SubAssign};

use num_traits::AsPrimitive;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Coord<T> {
    pub x: T,
    pub y: T,
}

impl<T: Copy> Coord<T> {
    pub fn new(x: T, y: T) -> Self {
        Coord { x, y }
    }
}

impl<T> Add for Coord<T>
where
    T: Add<Output = T>,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl<T> AddAssign for Coord<T>
where
    T: Copy + Add<Output = T>,
{
    fn add_assign(&mut self, other: Self) {
        *self = Self {
            x: self.x + other.x,
            y: self.y + other.y,
        };
    }
}

impl<T> Sub for Coord<T>
where
    T: Sub<Output = T>,
{
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl<T> SubAssign for Coord<T>
where
    T: Copy + Sub<Output = T>,
{
    fn sub_assign(&mut self, other: Self) {
        *self = Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl<T: AsPrimitive<i32>> Coord<T> {
    pub fn square_dist(&self, other: Coord<T>) -> i32 {
        let a = self.x.as_() - other.x.as_();
        let b = self.y.as_() - other.y.as_();
        (a * a) + (b * b)
    }
}
