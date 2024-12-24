use std::usize;

pub const PAGE_SIZE: usize = 4096;
pub const INVALID_PAGE_NUM: usize = 0;
pub type FrameId = usize;
pub type PageId = usize;
