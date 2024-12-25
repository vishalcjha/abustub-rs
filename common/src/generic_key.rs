#![allow(dead_code)]

#[repr(C)]
pub struct GenericKey<const N: usize> {
    key: [char; N],
}
