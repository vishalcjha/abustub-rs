#![allow(dead_code)]

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use async_trait::async_trait;
use common::PAGE_SIZE;

use super::page_operator::PageOperator;
pub(crate) struct DiskManager {
    db_path: PathBuf,
    db_file: File,
}

impl DiskManager {
    pub(super) fn new(path: &str) -> io::Result<DiskManager> {
        let path_buf = PathBuf::from(path);
        let file = OpenOptions::new().append(true).create(true).open(path)?;
        Ok(DiskManager {
            db_path: path_buf,
            db_file: file.into(),
        })
    }
}

#[async_trait]
impl PageOperator for DiskManager {
    fn write_page(
        &mut self,
        page_id: usize,
        data: Box<[u8; PAGE_SIZE]>,
    ) -> io::Result<Box<[u8; PAGE_SIZE]>> {
        let beginning_offset = page_id * PAGE_SIZE;
        self.db_file
            .seek(SeekFrom::Start(beginning_offset as u64))?;
        self.db_file.write_all(data.as_slice())?;
        self.db_file.flush()?;

        Ok(data)
    }

    fn read_page(
        &mut self,
        page_id: usize,
        mut data: Box<[u8; PAGE_SIZE]>,
    ) -> io::Result<Box<[u8; PAGE_SIZE]>> {
        let beginning_offset = page_id * PAGE_SIZE;
        self.db_file
            .seek(SeekFrom::Start(beginning_offset as u64))?;
        self.db_file.read_exact(data.as_mut_slice())?;
        Ok(data)
    }
}
