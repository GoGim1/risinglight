// Copyright 2022 RisingLight Project Authors. Licensed under Apache-2.0.

use std::iter::Sum;
use std::simd::{LaneCount, Simd, SimdElement, SupportedLaneCount};

use bitvec::prelude::{BitSlice, Lsb0};

use super::*;

impl<T: NativeType> PrimitiveArray<T> {
    /// Returns a batch iterator for SIMD.
    ///
    /// Each item contains at most `N` elements.
    pub fn batch_iter<const N: usize>(&self) -> BatchIter<'_, T, N> {
        assert!(N <= std::mem::size_of::<usize>() * 8);
        BatchIter {
            array: self,
            idx: 0,
        }
    }
}

/// An iterator over a batch elements of the array at a time.
pub struct BatchIter<'a, T: NativeType, const N: usize> {
    array: &'a PrimitiveArray<T>,
    idx: usize,
}

/// A batch elements generated by `BatchIter`.
#[derive(Debug, PartialEq, Eq)]
pub struct BatchItem<T, const N: usize>
where
    T: SimdElement + NativeType,
    LaneCount<N>: SupportedLaneCount,
{
    /// The elements.
    pub data: Simd<T, N>,
    /// The valid (non-NULL) bitmap.
    pub valid: usize,
    /// The length of the batch.
    pub len: usize,
}

impl<T, const N: usize> Iterator for BatchIter<'_, T, N>
where
    T: SimdElement + NativeType,
    LaneCount<N>: SupportedLaneCount,
{
    type Item = BatchItem<T, N>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.array.len() {
            return None;
        }
        let len = (self.array.len() - self.idx).min(N);
        let range = self.idx..self.idx + len;

        let mut valid = [0u8; std::mem::size_of::<usize>()];
        let bytes = (len + 7) >> 3;
        valid[..bytes].copy_from_slice(unsafe {
            std::slice::from_raw_parts(
                (self.array.valid.as_bitptr().pointer() as *const u8).add(self.idx >> 3),
                bytes,
            )
        });
        let valid = usize::from_le_bytes(valid);

        let data = if len == N {
            <[T; N]>::try_from(&self.array.data[range]).unwrap().into()
        } else {
            let mut data = Simd::<T, N>::default();
            data.as_mut_array()[..len].copy_from_slice(&self.array.data[range]);
            data
        };

        self.idx += N;
        Some(BatchItem { data, valid, len })
    }
}

impl<T, const N: usize> FromIterator<BatchItem<T, N>> for PrimitiveArray<T>
where
    T: SimdElement + NativeType,
    LaneCount<N>: SupportedLaneCount,
{
    fn from_iter<I: IntoIterator<Item = BatchItem<T, N>>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let mut builder = PrimitiveArrayBuilder::with_capacity(iter.size_hint().0 * N);
        for e in iter {
            builder
                .valid
                .extend_from_bitslice(&BitSlice::<usize, Lsb0>::from_element(&e.valid)[..e.len]);
            builder.data.extend_from_slice(&e.data[..e.len]);
        }
        builder.finish()
    }
}

macro_rules! impl_sum {
    ($($t:ty),*) => {$(
        impl<const N: usize> Sum<BatchItem<$t, N>> for $t
        where
            LaneCount<N>: SupportedLaneCount,
        {
            fn sum<I: Iterator<Item = BatchItem<$t, N>>>(iter: I) -> $t {
                iter.map(|batch| batch.data.reduce_sum()).sum()
            }
        }
    )*}
}
impl_sum!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_iter() {
        let a = (0..12)
            .map(|i| if i % 2 == 0 { Some(i) } else { None })
            .collect::<PrimitiveArray<u32>>();
        let mut iter = a.batch_iter::<8>();
        assert_eq!(
            iter.next(),
            Some(BatchItem {
                valid: 0b_0101_0101,
                data: [0, 0, 2, 0, 4, 0, 6, 0].into(),
                len: 8
            })
        );
        assert_eq!(
            iter.next(),
            Some(BatchItem {
                valid: 0b_0000_0101,
                data: [8, 0, 10, 0, 0, 0, 0, 0].into(),
                len: 4
            })
        );
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn batch_iter_collect() {
        let a = (0..12).collect::<PrimitiveArray<u32>>();
        let a1 = a.batch_iter::<8>().collect::<PrimitiveArray<u32>>();
        assert_eq!(a1, a);
    }

    #[test]
    fn batch_sum() {
        let a = (0..32).collect::<PrimitiveArray<i32>>();
        assert_eq!(a.batch_iter::<32>().sum::<i32>(), 496);
    }
}
