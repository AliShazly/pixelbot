use std::ops::{Add, AddAssign, Sub, SubAssign};

use num_traits::{AsPrimitive, Bounded};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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

impl<T> Coord<T> {
    pub fn bbox(coord_cluster: &[Coord<T>]) -> (Coord<T>, Coord<T>)
    where
        T: Copy + Ord + Bounded,
    {
        let mut x_max = T::min_value();
        let mut y_max = T::min_value();
        let mut x_min = T::max_value();
        let mut y_min = T::max_value();
        for coord in coord_cluster {
            if coord.x > x_max {
                x_max = coord.x;
            }
            if coord.y > y_max {
                y_max = coord.y;
            }
            if coord.x < x_min {
                x_min = coord.x;
            }
            if coord.y < y_min {
                y_min = coord.y;
            }
        }
        (Coord::new(x_min, y_min), Coord::new(x_max, y_max))
    }

    pub fn bbox_xywh(coord_cluster: &[Coord<T>]) -> (T, T, T, T)
    where
        T: Copy + Ord + Bounded + Sub<Output = T>,
    {
        let (min_coord, max_coord) = Self::bbox(coord_cluster);
        (
            min_coord.x,
            min_coord.y,
            max_coord.x - min_coord.x,
            max_coord.y - min_coord.y,
        )
    }
}
