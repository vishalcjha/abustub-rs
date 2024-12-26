#![allow(dead_code)]

use common::PAGE_SIZE;
use serde::{Deserialize, Serialize};

pub const PAGE_HEADER_SIZE: usize = std::mem::size_of::<PageHeader>();

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PageHeader {
    current_size: u16,
    max_size: u16,
}

impl PageHeader {
    pub fn new(data: &[u8; PAGE_SIZE], max_size: u16) -> PageHeader {
        let mut offset = 0;
        let current_size = u16::from_ne_bytes(data[offset..2].try_into().unwrap());
        offset += 2;
        let existing_max_size = u16::from_ne_bytes(data[offset..offset + 2].try_into().unwrap());
        // offset += 2;

        let max_size = if existing_max_size == 0 {
            max_size
        } else {
            existing_max_size
        };

        PageHeader {
            current_size,
            max_size,
        }
    }

    pub fn serialize(&self, data: &mut Box<[u8; PAGE_SIZE]>) {
        data[0..2].copy_from_slice(&u16::to_ne_bytes(self.current_size));
        data[2..4].copy_from_slice(&u16::to_ne_bytes(self.max_size));
    }

    pub fn set_current_size(&mut self, current_size: u16) {
        self.current_size = current_size;
    }

    pub fn get_current_size(&self) -> u16 {
        self.current_size
    }

    pub fn get_max_size(&self) -> u16 {
        self.max_size
    }
}

#[cfg(test)]
mod test {
    use crate::page::page_header::PageHeader;

    #[test]
    fn get_page_type_size() {
        println!("{}", std::mem::size_of::<PageHeader>());
        println!("{}", std::mem::size_of::<u16>());
    }

    #[test]
    fn test_header_change_current_size() {
        let mut data = Box::new([0; 4096]);
        let mut header = PageHeader::new(&data, 10);

        assert_eq!(0, header.current_size);
        assert_eq!(10, header.max_size);

        header.set_current_size(10);
        header.serialize(&mut data);

        let header = PageHeader::new(&data, 5);
        assert_eq!(10, header.current_size);
        // retain max size from last serialization
        assert_eq!(10, header.max_size);
    }
}
