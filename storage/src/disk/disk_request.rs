#![allow(dead_code)]

use std::io;

use common::{PageId, PAGE_SIZE};
use tokio::sync::oneshot;

pub enum DiskRequest {
    Read {
        page_id: PageId,
        data_buf: Vec<u8>,
        ack: oneshot::Sender<io::Result<()>>,
    },
    Write {
        page_id: PageId,
        data_buf: Vec<u8>,
        ack: oneshot::Sender<io::Result<()>>,
    },
}

impl DiskRequest {
    pub fn new_read(
        page_id: PageId,
        _data: &mut [u8; PAGE_SIZE],
    ) -> (Self, oneshot::Receiver<io::Result<()>>) {
        let (tx, rx) = oneshot::channel();
        let read = DiskRequest::Read {
            page_id,
            data_buf: Vec::new(),
            ack: tx,
        };

        (read, rx)
    }

    pub fn new_write(
        page_id: PageId,
        _data: &mut [u8; PAGE_SIZE],
    ) -> (Self, oneshot::Receiver<io::Result<()>>) {
        let (tx, rx) = oneshot::channel();
        let write = DiskRequest::Write {
            page_id,
            data_buf: Vec::new(),
            ack: tx,
        };

        (write, rx)
    }
}
