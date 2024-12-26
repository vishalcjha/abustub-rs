#![allow(dead_code)]
use crate::PageId;

pub struct Rid {
    page_id: PageId,
    slot_number: u32,
}

impl Rid {
    pub fn new(page_id: PageId, slot_number: u32) -> Self {
        Self {
            page_id,
            slot_number,
        }
    }
}
