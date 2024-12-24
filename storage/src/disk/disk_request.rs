#![allow(dead_code)]

use std::io;

use common::{PageId, PAGE_SIZE};
use tokio::sync::oneshot;

pub enum DiskRequest {
    Read {
        page_id: PageId,
        data_buf: Box<[u8; PAGE_SIZE]>,
        ack: oneshot::Sender<io::Result<Box<[u8; PAGE_SIZE]>>>,
    },
    Write {
        page_id: PageId,
        data_buf: Box<[u8; PAGE_SIZE]>,
        ack: oneshot::Sender<io::Result<Box<[u8; PAGE_SIZE]>>>,
    },
}

impl DiskRequest {
    pub(crate) fn new_read(
        page_id: PageId,
        data_buf: Box<[u8; PAGE_SIZE]>,
    ) -> (Self, oneshot::Receiver<io::Result<Box<[u8; PAGE_SIZE]>>>) {
        let (tx, rx) = oneshot::channel();
        let read = DiskRequest::Read {
            page_id,
            data_buf,
            ack: tx,
        };

        (read, rx)
    }

    pub(crate) fn new_write(
        page_id: PageId,
        data_buf: Box<[u8; PAGE_SIZE]>,
    ) -> (Self, oneshot::Receiver<io::Result<Box<[u8; PAGE_SIZE]>>>) {
        let (tx, rx) = oneshot::channel();
        let write = DiskRequest::Write {
            page_id,
            data_buf,
            ack: tx,
        };

        (write, rx)
    }
}
