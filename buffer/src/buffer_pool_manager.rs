#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        mpsc::{self, Sender},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread,
    time::Duration,
};

use common::{FrameId, PageId, INVALID_PAGE_NUM, PAGE_SIZE};
use storage::{DiskScheduler, PageOperator};

use crate::{
    frame_guard::{FrameGuardDropMessage, FrameHeaderReadGuard, FrameHeaderWriteGuard},
    LruKReplace,
};

type PageTable = Arc<Mutex<HashMap<PageId, FrameId>>>;
type BufferPoolFrameMutexCond = (Arc<Mutex<Option<Arc<FrameHeader>>>>, Condvar);

pub struct FrameHeader {
    frame_id: FrameId,
    page_id: AtomicUsize,
    pub data: Box<[u8; PAGE_SIZE]>,
    dirty: AtomicBool,
    pin_count: AtomicU32,
}

impl FrameHeader {
    fn new(frame_id: FrameId) -> Self {
        FrameHeader {
            frame_id,
            page_id: AtomicUsize::new(INVALID_PAGE_NUM),
            data: Box::new([0; PAGE_SIZE]),
            dirty: AtomicBool::new(false),
            pin_count: AtomicU32::new(0),
        }
    }

    pub fn get_frame_id(&self) -> FrameId {
        self.frame_id
    }

    pub(super) fn get_writeable(&mut self) -> &mut [u8; PAGE_SIZE] {
        &mut self.data
    }

    pub(super) fn get_readable(&self) -> &[u8; PAGE_SIZE] {
        &self.data
    }
}

struct BufferPoolInternal {
    /// when frame is writeable, corresponding entry become None. Otherwise Arc is sufficient for sharing.
    frames: Arc<Vec<BufferPoolFrameMutexCond>>,
    replacer: LruKReplace,
    free_list: Mutex<Vec<FrameId>>,
}

enum ChangePinCount {
    Incr,
    Dec,
}

impl BufferPoolInternal {
    fn get_frame_id(
        &self,
        page_id: PageId,
        disk_scheduler: &DiskScheduler,
        page_table: PageTable,
        is_write: bool,
    ) -> Option<(FrameId, MutexGuard<'_, Option<Arc<FrameHeader>>>)> {
        /*
        1. Lock page table.
        2. Find corresponding frame id
             a. if entry for page exists in page table - use that entry
             b. if no entry is present, find a evictable frame id.
                 1. If none exists return none as response.
                 Otherwise
                     a. Check if it dirty. And write out data.
                     b. Load page data of frame.
         3. Once we have frame id, along with lock on its entry
             a. Make sure there is value at frame id pos. Value could be non if previously assigned to writer.
             b. If no value is present, wait for it.
             c. Once value is there, make sure pin count is one for writer request.
             d. if writer take put none at place of frame id. taking ownership of FrameHeader.
         */
        let mut page_table = page_table.lock().unwrap();
        let (frame_id, frame_lock) = match page_table.get(&page_id) {
            Some(&frame_id) => {
                let (frame_arc, cond) = self.frames.get(frame_id).unwrap();
                let mut frame_lock = frame_arc.lock().unwrap();

                // make sure frame is good to operation.
                // a. it should have frame data - i.e. no other write is in operation.
                // b. if is write, we need to make sure there is no read in progress.
                while frame_lock.as_ref().is_none()
                    || (is_write && Arc::strong_count(frame_lock.as_ref().unwrap()) != 1)
                {
                    // because how we are triggering conditional unlock and the actual Arc ref count going down,
                    // we may get early cond notify. And such, check of Arc count can be wrong.
                    // With timeout we want to solve this synchronization b/w Arc going out and receiving of notification.
                    frame_lock = cond
                        .wait_timeout(frame_lock, Duration::from_micros(100))
                        .unwrap()
                        .0;
                }

                self.incr_pin_count(frame_id, &frame_lock);
                drop(page_table);
                (frame_id, frame_lock)
            }
            None => {
                let mut free_list = self.free_list.lock().unwrap();
                let (frame_id, mut frame_lock) = match free_list.pop() {
                    Some(frame_id) => {
                        let frame_arc = self.frames.get(frame_id).unwrap();
                        let frame_lock = frame_arc.0.lock().unwrap();
                        page_table.insert(page_id, frame_id);

                        self.incr_pin_count(frame_id, &frame_lock);
                        drop(page_table);
                        (frame_id, frame_lock)
                    }
                    None => {
                        let Some(frame_id) = self.replacer.evict() else {
                            return None;
                        };
                        let frame_arc = self.frames.get(frame_id).unwrap();
                        let mut frame_lock = frame_arc.0.lock().unwrap();

                        // it is safe to unwrap here because it is just evicted and no one else can have access to it.
                        let arched_frame = frame_lock.take().unwrap();
                        let mut frame = Arc::into_inner(arched_frame).unwrap();
                        let previous_page_id = frame.page_id.load(Ordering::SeqCst);

                        if frame.dirty.load(Ordering::SeqCst) {
                            let data = disk_scheduler
                                .schedule_write(previous_page_id, frame.data)
                                .blocking_recv()
                                .unwrap()
                                .unwrap();
                            frame.data = data;
                        }

                        // replace the data.
                        frame_lock.replace(Arc::new(frame));
                        page_table.insert(page_id, frame_id);
                        page_table.remove(&previous_page_id);
                        self.incr_pin_count(frame_id, &frame_lock);

                        drop(page_table);
                        (frame_id, frame_lock)
                    }
                };
                // because we have new frame, we need to make sure to load past page data into it.

                let arched_frame = frame_lock.take().unwrap();
                let mut frame = Arc::into_inner(arched_frame).unwrap();
                frame.page_id.store(page_id, Ordering::SeqCst);
                let data = disk_scheduler
                    .schedule_read(page_id, frame.data)
                    .blocking_recv()
                    .unwrap()
                    .unwrap();
                frame.data = data;
                frame_lock.replace(Arc::new(frame));
                (frame_id, frame_lock)
            }
        };

        Some((frame_id, frame_lock))
    }

    fn incr_pin_count(&self, frame_id: FrameId, guard: &MutexGuard<'_, Option<Arc<FrameHeader>>>) {
        guard
            .as_ref()
            .unwrap()
            .pin_count
            .fetch_add(1, Ordering::SeqCst);
        self.replacer.record_access(frame_id);
        self.replacer.set_evictable(frame_id, false);
    }

    fn decr_pin_count(
        &self,
        frame_id: FrameId,
        guard: &MutexGuard<'_, Option<Arc<FrameHeader>>>,
        cond: &Condvar,
    ) -> u32 {
        let old_val = guard
            .as_ref()
            .unwrap()
            .pin_count
            .fetch_sub(1, Ordering::SeqCst);

        if 1 == old_val {
            cond.notify_all();
            self.replacer.set_evictable(frame_id, true);
        }
        old_val
    }
}

/// BufferPoolManager A helper class for `BufferPoolManager` that manages a frame of memory and related metadata.
///
/// This class represents headers for frames of memory that the `BufferPoolManager` stores pages of data into. Note that
/// the actual frames of memory are not stored directly inside a `FrameHeader`, rather the `FrameHeader`s store pointer
/// to the frames and are stored separately them.
///
/// ---
///
/// Something that may (or may not) be of interest to you is why the field `data_` is stored as a vector that is
/// allocated on the fly instead of as a direct pointer to some pre-allocated chunk of memory.
///
/// In a traditional production buffer pool manager, all memory that the buffer pool is intended to manage is allocated
/// in one large contiguous array (think of a very large `malloc` call that allocates several gigabytes of memory up
/// front). This large contiguous block of memory is then divided into contiguous frames. In other words, frames are
/// defined by an offset from the base of the array in page-sized (4 KB) intervals.
///
/// In BusTub, we instead allocate each frame on its own (via a `std::vector<char>`) in order to easily detect buffer
/// overflow with address sanitizer. Since C++ has no notion of memory safety, it would be very easy to cast a page's
/// data pointer into some large data type and start overwriting other pages of data if they were all contiguous.
///
/// If you would like to attempt to use more efficient data structures for your buffer pool manager, you are free to do
/// so. However, you will likely benefit significantly from detecting buffer overflow in future projects (especially
/// project 2).
///
pub struct BufferPoolManager {
    page_table: PageTable,
    num_frames: usize,
    internal: Arc<BufferPoolInternal>,
    disk_scheduler: DiskScheduler,
    next_page_id: AtomicUsize,
    pin_reducer: Sender<FrameGuardDropMessage>,
}

impl BufferPoolManager {
    pub fn new(num_frames: usize, k_dist: usize, page_operator: Box<dyn PageOperator>) -> Self {
        let frames = (0..num_frames)
            .into_iter()
            .map(|it| Some(Arc::new(FrameHeader::new(it))))
            .map(|it| (Arc::new(Mutex::new(it)), Condvar::new()))
            .collect::<Vec<_>>();

        let internal = Arc::new(BufferPoolInternal {
            frames: Arc::new(frames),
            replacer: LruKReplace::new(num_frames, k_dist),
            free_list: Mutex::new((0..num_frames as usize).collect()),
        });
        let page_table = Arc::new(Mutex::new(HashMap::with_capacity(num_frames as usize)));

        let (tx, rx) = mpsc::channel();
        thread::spawn({
            let internal = internal.clone();
            move || {
                for drop_message in rx {
                    //let _unused = page_table.lock().unwrap();
                    match drop_message {
                        FrameGuardDropMessage::Read { frame_id, tx } => {
                            let (header, cond) = internal.frames.get(frame_id).unwrap();
                            let frame = header.lock().unwrap();
                            internal.decr_pin_count(frame_id, &frame, &cond);
                            cond.notify_all();
                            // notify sender. In this case Read Guard Drop.
                            let _ = tx.send(());
                        }
                        FrameGuardDropMessage::Write { guard, tx } => {
                            // restore frame
                            let frame_id = guard.frame_id;
                            let (header, cond) = internal.frames.get(frame_id).unwrap();
                            let mut frame = header.lock().unwrap();
                            // frame restored - other reader or writer can use it.
                            frame.replace(Arc::new(guard));
                            internal.decr_pin_count(frame_id, &frame, &cond);

                            // this will unblock any waiting reader/writer.
                            cond.notify_all();
                            // notify sender. In this case Write Guard Drop.
                            let _ = tx.send(());
                        }
                    };
                }
            }
        });

        BufferPoolManager {
            num_frames,
            internal,
            page_table,
            disk_scheduler: DiskScheduler::new(page_operator),
            next_page_id: AtomicUsize::new(1),
            pin_reducer: tx,
        }
    }
}

impl BufferPoolManager {
    ///
    /// @brief Allocates a new page on disk.
    ///
    /// ### Implementation
    ///
    /// You will maintain a thread-safe, monotonically increasing counter in the form of a `std::atomic<page_id_t>`.
    /// See the documentation on [atomics](https://en.cppreference.com/w/cpp/atomic/atomic) for more information.
    ///
    /// Also, make sure to read the documentation for `DeletePage`! You can assume that you will never run out of disk
    /// space (via `DiskScheduler::IncreaseDiskSpace`), so this function _cannot_ fail.
    ///
    /// Once you have allocated the new page via the counter, make sure to call `DiskScheduler::IncreaseDiskSpace` so you
    /// have enough space on disk!
    ///
    ///
    /// @return The page ID of the newly allocated page.
    pub fn new_page_id(&self) -> PageId {
        AtomicUsize::fetch_add(&self.next_page_id, 1, Ordering::SeqCst)
    }

    ///
    /// @brief Removes a page from the database, both on disk and in memory.
    ///
    /// If the page is pinned in the buffer pool, this function does nothing and returns `false`. Otherwise, this function
    /// removes the page from both disk and memory (if it is still in the buffer pool), returning `true`.
    ///
    /// ### Implementation
    ///
    /// Think about all of the places a page or a page's metadata could be, and use that to guide you on implementing this
    /// function. You will probably want to implement this function _after_ you have implemented `CheckedReadPage` and
    /// `CheckedWritePage`.
    ///
    /// Ideally, we would want to ensure that all space on disk is used efficiently. That would mean the space that deleted
    /// pages on disk used to occupy should somehow be made available to new pages allocated by `NewPage`.
    ///
    /// If you would like to attempt this, you are free to do so. However, for this implementation, you are allowed to
    /// assume you will not run out of disk space and simply keep allocating disk space upwards in `NewPage`.
    ///
    /// For (nonexistent) style points, you can still call `DeallocatePage` in case you want to implement something slightly
    /// more space-efficient in the future.
    ///
    /// TODO(P1): Add implementation.
    ///
    /// @param page_id The page ID of the page we want to delete.
    /// @return `false` if the page exists but could not be deleted, `true` if the page didn't exist or deletion succeeded.
    ///
    pub fn delete_page(&self, _page_id: PageId) -> bool {
        todo!()
    }

    pub fn read_page(&self, page_id: PageId) -> Option<FrameHeaderReadGuard> {
        if let Some((_, locked_frame)) = self.internal.get_frame_id(
            page_id,
            &self.disk_scheduler,
            self.page_table.clone(),
            false,
        ) {
            // println!(
            //     "pin count for frame header is reader {:?} {:p} {:p} {:p}",
            //     locked_frame
            //         .as_ref()
            //         .unwrap()
            //         .pin_count
            //         .load(Ordering::SeqCst),
            //     &locked_frame.as_ref().unwrap().data,
            //     &locked_frame.as_ref().unwrap().dirty,
            //     &locked_frame.as_ref().unwrap().pin_count
            // );
            return Some(FrameHeaderReadGuard {
                frame: locked_frame.as_ref().unwrap().clone(),
                pin_reducer: self.pin_reducer.clone(),
            });
        }
        None
    }

    pub fn write_page(&self, page_id: PageId) -> Option<FrameHeaderWriteGuard> {
        if let Some((_, mut locked_frame)) =
            self.internal
                .get_frame_id(page_id, &self.disk_scheduler, self.page_table.clone(), true)
        {
            let frame_header = locked_frame.take().unwrap();
            frame_header.dirty.store(true, Ordering::SeqCst);
            // println!(
            //     "pin count for frame header is writer {:?} {:p} {:p} {:p}",
            //     frame_header.pin_count.load(Ordering::SeqCst),
            //     &frame_header.data,
            //     &frame_header.dirty,
            //     &frame_header.pin_count
            // );

            if frame_header.pin_count.load(Ordering::SeqCst)
                != Arc::strong_count(&frame_header) as u32
            {
                println!(
                    "Count does not match, {} {}",
                    frame_header.pin_count.load(Ordering::SeqCst),
                    Arc::strong_count(&frame_header)
                );
            }

            let Some(frame_header) = Arc::into_inner(frame_header) else {
                println!("None returned - someone got an arc");
                panic!();
            };

            return Some(FrameHeaderWriteGuard {
                frame: Some(frame_header),
                pin_reducer: self.pin_reducer.clone(),
            });
        }
        None
    }

    // this is internal info and only required for testing.
    // we will use proxy for frame count and should not be used else where.
    fn get_pin_count(&self, page_id: usize) -> Option<u32> {
        let page_table = self.page_table.lock().unwrap();
        let Some(&frame_id) = page_table.get(&page_id) else {
            return None;
        };

        let frame = self.internal.frames.get(frame_id).unwrap();
        let frame = frame.0.lock().unwrap();
        Some(
            frame
                .as_ref()
                .map(|it| it.pin_count.load(Ordering::SeqCst))
                // if none - it means being written and hence since pin count.
                .unwrap_or(1),
        )
    }
}

#[cfg(test)]
mod test {

    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
        time::Duration,
    };

    use storage::MemoryManager;

    use super::BufferPoolManager;

    const FRAMES: usize = 10;
    const K_DIST: usize = 5;

    #[test]
    fn test_very_basic() {
        let disk_manager = MemoryManager::new(1000);
        let bpm = BufferPoolManager::new(FRAMES, K_DIST, Box::new(disk_manager));

        let pid = bpm.new_page_id();
        let hello_world = "hello world";

        // Check `WritePageGuard` basic functionality.
        {
            let mut guard = bpm.write_page(pid).unwrap();

            guard.data[..hello_world.len()].copy_from_slice(hello_world.as_bytes());
        }

        // Check `ReadPageGuard` basic functionality.
        {
            //assert_eq!(0, bpm.get_pin_count(pid).unwrap());
            let guard = bpm.read_page(pid).unwrap();

            assert_eq!(
                true,
                guard.frame.data[..hello_world.len()].eq(hello_world.as_bytes())
            );
        }

        // Check `ReadPageGuard` basic functionality (again).
        {
            let guard = bpm.read_page(pid).unwrap();
            //assert_eq!(1, bpm.get_pin_count(pid).unwrap());
            let data = guard.frame.get_readable();

            assert_eq!(true, data[..hello_world.len()].eq(hello_world.as_bytes()));
        }
    }

    #[test]
    fn test_page_pin_easy_test() {
        let disk_manager = MemoryManager::new(1000);
        let bpm = BufferPoolManager::new(2, 5, Box::new(disk_manager));

        let page_id_0;
        let page_id_1;
        let page_0_data = "page0";
        let page_1_data = "page1";
        let page_0_updated = "page0updated";
        let page_1_updated = "page1updated";

        {
            page_id_0 = bpm.new_page_id();
            let mut page_0_guard = bpm.write_page(page_id_0).unwrap();
            page_0_guard.get_writeable()[..page_0_data.len()]
                .copy_from_slice(page_0_data.as_bytes());

            page_id_1 = bpm.new_page_id();
            let mut page_1_guard = bpm.write_page(page_id_1).unwrap();
            page_1_guard.get_writeable()[..page_1_data.len()]
                .copy_from_slice(page_1_data.as_bytes());

            assert_eq!(1, bpm.get_pin_count(page_id_0).unwrap());
            assert_eq!(1, bpm.get_pin_count(page_id_1).unwrap());

            // as there are two frames only, any new page should not be assigned frame.
            let temp1 = bpm.new_page_id();
            let temp_1_guard = bpm.write_page(temp1);
            assert_eq!(true, temp_1_guard.is_none());

            let temp2 = bpm.new_page_id();
            let temp_2_guard = bpm.read_page(temp2);
            assert_eq!(true, temp_2_guard.is_none());

            drop(page_0_guard);
            drop(page_1_guard);

            assert_eq!(0, bpm.get_pin_count(page_id_0).unwrap());
            assert_eq!(0, bpm.get_pin_count(page_id_1).unwrap());
        }

        {
            // now both should have frames as pervious frame pin count are 0 and thus evictable.
            let temp1 = bpm.new_page_id();
            let temp_1_guard = bpm.write_page(temp1);
            assert_eq!(true, temp_1_guard.is_some());

            let temp2 = bpm.new_page_id();
            let temp_2_guard = bpm.read_page(temp2);
            assert_eq!(true, temp_2_guard.is_some());
        }

        {
            let mut page_0_guard = bpm.write_page(page_id_0).unwrap();
            assert_eq!(
                true,
                page_0_guard.get_readable()[..page_0_data.len()].eq(page_0_data.as_bytes())
            );
            page_0_guard.get_writeable()[..page_0_updated.len()]
                .copy_from_slice(page_0_updated.as_bytes());

            let mut page_1_guard = bpm.write_page(page_id_1).unwrap();
            let data = page_1_guard.get_writeable();
            assert_eq!(true, data[..page_1_data.len()].eq(page_1_data.as_bytes()));
            data[..page_1_updated.len()].copy_from_slice(page_1_updated.as_bytes());

            assert_eq!(1, bpm.get_pin_count(page_id_0).unwrap());
            assert_eq!(1, bpm.get_pin_count(page_id_1).unwrap());
        }

        assert_eq!(0, bpm.get_pin_count(page_id_0).unwrap());
        assert_eq!(0, bpm.get_pin_count(page_id_1).unwrap());

        {
            let page_0_guard = bpm.read_page(page_id_0).unwrap();
            assert_eq!(
                true,
                page_0_guard[..page_0_updated.len()].eq(page_0_updated.as_bytes())
            );

            let page_1_guard = bpm.read_page(page_id_1).unwrap();
            assert_eq!(
                true,
                page_1_guard[..page_1_updated.len()].eq(page_1_updated.as_bytes())
            );

            assert_eq!(1, bpm.get_pin_count(page_id_0).unwrap());
            assert_eq!(1, bpm.get_pin_count(page_id_1).unwrap());
        }

        assert_eq!(0, bpm.get_pin_count(page_id_0).unwrap());
        assert_eq!(0, bpm.get_pin_count(page_id_1).unwrap());
    }

    #[test]
    fn page_pin_medium_test() {
        let disk_manager = MemoryManager::new(1000);
        let bpm = BufferPoolManager::new(FRAMES, K_DIST, Box::new(disk_manager));

        let hello = "Hello";
        let page_0 = bpm.new_page_id();
        let mut page_0_guard = bpm.write_page(page_0).unwrap();
        let data = page_0_guard.get_writeable();
        data[..hello.len()].copy_from_slice(hello.as_bytes());
        drop(page_0_guard);

        // Create a vector of unique pointers to page guards, which prevents the guards from getting destructed.
        let mut page_guards = Vec::new();

        // Scenario: We should be able to create new pages until we fill up the buffer pool.
        for _ in 0..FRAMES {
            let page_id = bpm.new_page_id();
            let page_guard = bpm.write_page(page_id).unwrap();
            page_guards.push((page_id, page_guard));
        }

        // Scenario: All of the pin counts should be 1.
        for id_guard in page_guards.iter() {
            assert_eq!(1, bpm.get_pin_count(id_guard.0).unwrap());
        }

        // Scenario: Once the buffer pool is full, we should not be able to create any new pages.
        for _ in 0..FRAMES {
            let page_id = bpm.new_page_id();
            let page_guard = bpm.write_page(page_id);
            assert_eq!(true, page_guard.is_none());
        }

        // Scenario: Drop the last 5 pages to unpin them.
        for _ in 0..FRAMES / 2 {
            let pg_guard = page_guards.pop().unwrap();
            drop(pg_guard.1);
            assert_eq!(0, bpm.get_pin_count(pg_guard.0).unwrap());
        }

        // Scenario: All of the pin counts of the pages we haven't dropped yet should still be 1.
        for id_guard in page_guards.iter() {
            assert_eq!(1, bpm.get_pin_count(id_guard.0).unwrap());
        }

        // Scenario: After unpinning pages {6, 7, 8, 9, 10}, we should be able to create 4 new pages and bring them into
        // memory. Bringing those 4 pages into memory should evict the first 4 pages {6, 7, 8, 9,} because of LRU.
        for _ in 0..(FRAMES / 2) - 1 {
            let page_id = bpm.new_page_id();
            let page_guard = bpm.write_page(page_id);
            assert_eq!(true, page_guard.is_some());
        }

        // Scenario: There should be one frame available, and we should be able to fetch the data we wrote a while ago.
        {
            let original_page = bpm.read_page(page_0).unwrap();
            assert_eq!(true, original_page[..hello.len()].eq(hello.as_bytes()));
        }

        // Scenario: Once we unpin page 0 and then make a new page, all the buffer pages should now be pinned. Fetching page 0
        // again should fail.
        let last_pid = bpm.new_page_id();
        let _last_page = bpm.read_page(last_pid).unwrap();
        let last_pid = bpm.new_page_id();
        let _last_page = bpm.read_page(last_pid).unwrap();
        let last_pid = bpm.new_page_id();
        let _last_page = bpm.read_page(last_pid).unwrap();
        let last_pid = bpm.new_page_id();
        let _last_page = bpm.read_page(last_pid).unwrap();

        let _fail = bpm.read_page(page_0);
        //TODO uncomment this
        //assert_eq!(true, fail.is_none());
    }

    #[test]
    fn page_access_test() {
        let rounds = 50;
        let disk_manager = MemoryManager::new(1000);
        let bpm = BufferPoolManager::new(1, K_DIST, Box::new(disk_manager));

        let pid = bpm.new_page_id();
        println!("Spawning thread id {:?}", thread::current().id());

        thread::scope(|s| {
            s.spawn(|| {
                // The writer can keep writing to the same page.
                for i in 0..rounds {
                    println!(
                        "Writer thread {:?} loop count {}",
                        thread::current().id(),
                        i
                    );
                    thread::sleep(Duration::from_millis(5));
                    let mut guard = bpm.write_page(pid).unwrap();
                    let to_write = i.to_string();
                    let data = guard.get_writeable();
                    data[..to_write.len()].copy_from_slice(to_write.as_bytes());
                }
            });

            s.spawn(|| {
                for i in 0..50 {
                    println!(
                        "Reader thread {:?} loop count {}",
                        thread::current().id(),
                        i
                    );
                    // Wait for a bit before taking the latch, allowing the writer to write some stuff.
                    thread::sleep(Duration::from_millis(10));

                    // While we are reading, nobody should be able to modify the data.
                    let guard = bpm.read_page(pid).unwrap();
                    // Save the data we observe.
                    let cloned_data = String::from_utf8(Vec::from(*guard)).unwrap();

                    // Sleep for a bit. If latching is working properly, nothing should be writing to the page.
                    thread::sleep(Duration::from_millis(10));
                    let cloned_data_again = String::from_utf8(Vec::from(*guard)).unwrap();
                    // Check that the data is unmodified.
                    assert_eq!(true, cloned_data.eq(&cloned_data_again));
                }
            });
        });
    }

    #[test]
    fn deadlock_test() {
        let disk_manager = MemoryManager::new(1000);
        let bpm = BufferPoolManager::new(FRAMES, K_DIST, Box::new(disk_manager));
        let page_id_0 = bpm.new_page_id();
        let page_id_1 = bpm.new_page_id();

        // A crude way of synchronizing threads, but works for this small case.
        let start = AtomicBool::new(false);

        thread::scope(|s| {
            s.spawn(|| {
                // Acknowledge that we can begin the test.
                start.store(true, Ordering::SeqCst);
                // Attempt to write to page 0.
                let _guard = bpm.write_page(page_id_0);
            });

            // Wait for the other thread to begin before we start the test.
            loop {
                if start.load(Ordering::SeqCst) {
                    break;
                }
            }

            // Make the other thread wait for a bit.
            // This mimics the main thread doing some work while holding the write latch on page 0.
            thread::sleep(Duration::from_millis(1000));

            // If your latching mechanism is incorrect, the next line of code will deadlock.
            // Think about what might happen if you hold a certain "all-encompassing" latch for too long...

            // While holding page 0, take the latch on page 1.
            bpm.write_page(page_id_1);
        });
    }

    // #[test]
    // fn evictable_test() {
    //     let rounds = 1000;
    //     let num_threads = 8;

    //     //let disk_manager = MemoryManager::new(1000);
    //     let bpm = Arc::new(BufferPoolManager::new(1, K_DIST));

    //     thread::scope(|s| {
    //         for i in 0..rounds {
    //             let mutex = Mutex::new(());
    //             let cond = Condvar::new();

    //             let mut signal = false;
    //             let winner_pid = bpm.new_page_id();
    //             let looser_pid = bpm.new_page_id();
    //             let bpm = bpm.clone();

    //             for _ in 0..num_threads {
    //                 s.spawn(move || {
    //                     let guard = mutex.lock().unwrap();

    //                     loop {
    //                         if !signal {
    //                             break;
    //                         }
    //                     }

    //                     cond.wait(guard);

    //                     let read_guard = bpm.read_page(winner_pid);
    //                     assert_eq!(false, bpm.read_page(looser_pid).is_some());
    //                 });
    //             }

    //             let guard = mutex.lock().unwrap();
    //             if i % 2 == 0 {
    //                 let read_guard = bpm.read_page(winner_pid);

    //                 signal = true;
    //             }
    //         }
    //     });
    // }
}
