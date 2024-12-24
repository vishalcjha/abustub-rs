#![allow(dead_code)]

use std::{
    io,
    thread::{self, JoinHandle},
};

use common::{PageId, PAGE_SIZE};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedSender},
    oneshot::Receiver,
};

use super::{disk_request::DiskRequest, page_operator::PageOperator};
pub struct DiskScheduler {
    worker_handler: JoinHandle<()>,
    work_queue: UnboundedSender<DiskRequest>,
}

impl DiskScheduler {
    pub fn new(mut page_operator: Box<dyn PageOperator>) -> Self {
        let (tx, mut rx) = unbounded_channel::<DiskRequest>();
        let worker_handler = thread::spawn(move || {
            while let Some(work) = rx.blocking_recv() {
                match work {
                    DiskRequest::Read {
                        page_id,
                        data_buf,
                        ack,
                    } => {
                        let result = page_operator.read_page(page_id, data_buf);
                        let _ = ack.send(result);
                    }
                    DiskRequest::Write {
                        page_id,
                        data_buf,
                        ack,
                    } => {
                        let result = page_operator.write_page(page_id, data_buf);
                        let _ = ack.send(result);
                    }
                }
            }
        });
        DiskScheduler {
            worker_handler,
            work_queue: tx,
        }
    }

    pub fn schedule_read(
        &self,
        page_id: PageId,
        data_buf: Box<[u8; PAGE_SIZE]>,
    ) -> Receiver<io::Result<Box<[u8; PAGE_SIZE]>>> {
        let (request, receiver) = DiskRequest::new_read(page_id, data_buf);
        let _ = self.work_queue.send(request);
        receiver
    }

    pub fn schedule_write(
        &self,
        page_id: PageId,
        data_buf: Box<[u8; PAGE_SIZE]>,
    ) -> Receiver<io::Result<Box<[u8; PAGE_SIZE]>>> {
        let (request, receiver) = DiskRequest::new_write(page_id, data_buf);
        let _ = self.work_queue.send(request);
        receiver
    }
}
