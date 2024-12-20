#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use common::{FrameId, PageId, PAGE_SIZE};
use storage::DiskScheduler;

use crate::LruKReplace;

#[derive(Clone)]
struct FrameHeader {
    frame_id: FrameId,
    data: Arc<RwLock<[u8; PAGE_SIZE]>>,
}

impl FrameHeader {
    fn new(frame_id: FrameId) -> Self {
        FrameHeader {
            frame_id,
            data: Arc::new(RwLock::new([0; PAGE_SIZE])),
        }
    }
}

struct BufferPoolManager {
    num_frames: u32,
    frames: Arc<Vec<FrameHeader>>,
    page_table: Arc<Mutex<HashMap<PageId, FrameId>>>,
    disk_scheduler: DiskScheduler,
    replacer: LruKReplace,
}

impl BufferPoolManager {
    pub fn new(num_frames: u32, k_dist: usize) -> Self {
        BufferPoolManager {
            num_frames,
            frames: Arc::new(
                (0..num_frames)
                    .into_iter()
                    .map(|it| FrameHeader::new(it as usize))
                    .collect(),
            ),
            replacer: LruKReplace::new(num_frames, k_dist),
            disk_scheduler: DiskScheduler::new(),
            page_table: Arc::new(Mutex::new(HashMap::with_capacity(num_frames as usize))),
        }
    }
}
