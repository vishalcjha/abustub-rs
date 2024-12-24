#![allow(dead_code)]

use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use common::PAGE_SIZE;

use super::page_operator::PageOperator;

pub struct MemoryManager {
    page_capacity: usize,
    memory: Cursor<Vec<u8>>,
}

impl MemoryManager {
    pub fn new(page_capacity: usize) -> Self {
        let memory = vec![0u8; page_capacity * PAGE_SIZE];
        Self {
            page_capacity,
            memory: Cursor::new(memory),
        }
    }

    fn assert_page_bound(&self, page_id: usize) {
        if page_id >= self.page_capacity {
            panic!(
                "Memory page manager can only full fill page_id below {} but requested {}",
                self.page_capacity, page_id
            );
        }
    }
}

impl PageOperator for MemoryManager {
    fn write_page(
        &mut self,
        page_id: usize,
        data: Box<[u8; PAGE_SIZE]>,
    ) -> std::io::Result<Box<[u8; PAGE_SIZE]>> {
        self.assert_page_bound(page_id);
        let pos = page_id * PAGE_SIZE;
        self.memory.seek(SeekFrom::Start(pos as u64))?;
        self.memory.write_all(data.as_slice())?;
        Ok(data)
    }

    fn read_page(
        &mut self,
        page_id: usize,
        mut data: Box<[u8; PAGE_SIZE]>,
    ) -> std::io::Result<Box<[u8; PAGE_SIZE]>> {
        self.assert_page_bound(page_id);
        let pos = page_id * PAGE_SIZE;
        self.memory.seek(SeekFrom::Start(pos as u64))?;
        self.memory.read_exact(data.as_mut_slice())?;
        Ok(data)
    }
}
