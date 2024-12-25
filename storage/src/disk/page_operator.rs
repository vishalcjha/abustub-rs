#![allow(dead_code)]

use common::PAGE_SIZE;

pub trait PageOperator: Send {
    fn write_page(
        &mut self,
        page_id: usize,
        data: Box<[u8; PAGE_SIZE]>,
    ) -> common::Result<Box<[u8; PAGE_SIZE]>>;

    fn read_page(
        &mut self,
        page_id: usize,
        data: Box<[u8; PAGE_SIZE]>,
    ) -> common::Result<Box<[u8; PAGE_SIZE]>>;
}
