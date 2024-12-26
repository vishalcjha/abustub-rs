use std::{result, usize};
mod data_type;
mod data_type_parse;
mod error;
mod field;
mod generic_key;
mod rid;

pub type MutablePage<'a> = &'a mut Box<[u8; 4096]>;
pub type ReadOnlyPage<'a> = &'a [u8; 4096];
pub const PAGE_SIZE: usize = 4096;
pub const INVALID_PAGE_NUM: usize = 0;
pub type FrameId = usize;
pub type PageId = usize;
pub type Result<T> = result::Result<T, BustubError>;

pub use error::BustubError;
pub use generic_key::GenericKey;
pub use rid::Rid;
