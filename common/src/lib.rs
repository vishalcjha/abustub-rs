use std::{result, usize};
mod data_type;
mod data_type_parse;
mod error;
mod generic_key;
mod rid;

pub const PAGE_SIZE: usize = 4096;
pub const INVALID_PAGE_NUM: usize = 0;
pub type FrameId = usize;
pub type PageId = usize;
pub type Result<T> = result::Result<T, BustubError>;

pub use error::BustubError;
pub use rid::Rid;
