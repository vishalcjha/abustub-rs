#![allow(dead_code)]

use std::slice;

use crate::{MutablePage, ReadOnlyPage};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct GenericKey<const N: usize> {
    key: [u8; N],
}

macro_rules! impl_into {
    ($N:literal, $primitive_type:ty) => {
        impl Into<$primitive_type> for GenericKey<$N> {
            fn into(self) -> $primitive_type {
                <$primitive_type>::from_ne_bytes(self.key)
            }
        }
    };
}
macro_rules! impl_from {
    ($N:literal, $primitive_type:ty) => {
        impl From<$primitive_type> for GenericKey<$N> {
            fn from(value: $primitive_type) -> Self {
                let mut key = GenericKey { key: [0; $N] };
                key.key.copy_from_slice(&value.to_ne_bytes());
                key
            }
        }
    };
}

// macro_rules! make_updater {
//     ($N:literal, $primitive_type:ty) => {
//         impl GenericKey<$N> {
//             pub fn update(&mut self, value: $primitive_type) {
//                 self.key
//                     .copy_from_slice(&value.to_ne_bytes().map(|it| it as char));
//             }
//         }
//     };
// }

impl<const N: usize> GenericKey<N> {
    pub fn from_raw_parts<'a>(
        page: ReadOnlyPage,
        offset: usize,
        key_count: usize,
    ) -> &'a [GenericKey<N>] {
        let initial = page.as_ptr();
        let start_of_generic_key = unsafe { initial.add(offset) } as *const GenericKey<N>;

        unsafe { slice::from_raw_parts(start_of_generic_key, key_count) }
    }

    pub fn from_raw_parts_mut<'a>(
        page: MutablePage,
        offset: usize,
        key_count: usize,
    ) -> &'a mut [GenericKey<N>] {
        let initial = page.as_mut_ptr();
        let start_of_generic_key = unsafe { initial.add(offset) } as *mut GenericKey<N>;
        let is_aligned = start_of_generic_key.is_aligned();
        dbg!("Is already aligned {}", is_aligned);

        unsafe { slice::from_raw_parts_mut(start_of_generic_key, key_count) }
    }
}

impl_from!(1, u8);
impl_from!(2, u16);
impl_from!(4, u32);
impl_from!(8, u64);

impl_into!(1, u8);
impl_into!(2, u16);
impl_into!(4, u32);
impl_into!(8, u64);

#[cfg(test)]
mod test {
    use super::GenericKey;

    #[test]
    fn test_write_to_generic_key() {
        let key = Into::<GenericKey<4>>::into(1024 as u32);
        println!("{:?}", key);
        let key: u32 = key.into();
        println!("{:?}", key);
    }

    #[test]
    fn to_ne() {
        println!("{:?}", u32::to_ne_bytes(21));
    }
}
