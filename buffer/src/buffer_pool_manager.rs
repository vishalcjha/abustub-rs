#![allow(dead_code)]

use std::{
    collections::HashMap, hash::Hash, sync::{atomic::{AtomicU32, AtomicUsize, Ordering}, mpsc::{self, Sender}, Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard}, thread
};

use common::{FrameId, PageId, PAGE_SIZE};
use storage::DiskScheduler;

use crate::LruKReplace;

#[derive(Clone)]
struct FrameHeader {
    frame_id: FrameId,
    data: [u8; PAGE_SIZE],
    dirty: bool,
}

impl FrameHeader {
    fn new(frame_id: FrameId) -> Self {
        FrameHeader {
            frame_id,
            data: [0; PAGE_SIZE],
            dirty: false,
        }
    }
}

struct FrameHeaderReadGuard {
    frame: Arc<RwLock<FrameHeader>>,
    pin_reducer: Sender<FrameId>,
}

struct FrameHeaderWriteGuard {
    frame: Arc<RwLock<FrameHeader>>,
    pin_reducer: Sender<FrameId>,
}

impl FrameHeaderWriteGuard {
    fn new(frame: Arc<RwLock<FrameHeader>>, pin_reducer: Sender<FrameId>) -> Self {
        FrameHeaderWriteGuard { frame, pin_reducer }
    }
}

impl Drop for FrameHeaderWriteGuard {
    fn drop(&mut self) {
        //self.pin_reducer.send(t)
    }
}

impl FrameHeaderReadGuard {
    fn new(frame: Arc<RwLock<FrameHeader>>, pin_reducer: Sender<FrameId>,) -> Self {
        FrameHeaderReadGuard { frame, pin_reducer }
    }
}


struct BufferPoolInternal {
    frames: Vec<Arc<RwLock<FrameHeader>>>,
    page_table: HashMap<PageId, FrameId>,
    replacer: LruKReplace,
    free_list: Vec<FrameId>,
    frame_pin_count: Vec<AtomicU32>,
}

impl BufferPoolInternal {
    fn get_frame_id(&mut self, page_id: PageId, disk_scheduler: &DiskScheduler) -> Option<FrameId> {
        if let Some(&frame_id) = self.page_table.get(&page_id) {
            self.replacer.record_access(frame_id);
            self.replacer.set_evictable(frame_id, false);
            self.frame_pin_count[frame_id].fetch_add(1, Ordering::SeqCst);
            return Some(frame_id);
        }

        if self.free_list.is_empty() {
            let Some(frame_id) =  self.replacer.evict() else {
                return None;
            };
            let frame = &mut self.frames[frame_id].write().unwrap();
            // if frame just got evicted, there is no lock on it.
            if frame.dirty{
                //TODO write the dirty page to disk
                //disk_scheduler(page_id, &frame.data.read().unwrap());
            }

            frame.dirty = false;
            self.free_list.push(frame_id);
        }

        // tie the page to frame.
        if let Some (frame_id) = self.free_list.pop() {
            self.page_table.insert(page_id, frame_id);
            self.replacer.record_access(frame_id);
            self.replacer.set_evictable(frame_id, false);
            self.frame_pin_count[frame_id].fetch_add(1, Ordering::SeqCst);
            return Some(frame_id);
        }

        None
    }
}

struct BufferPoolManager {
    num_frames: usize,
    internal: Arc<Mutex<BufferPoolInternal>>,
    disk_scheduler: DiskScheduler,
    next_page_id: AtomicUsize,
    pin_reducer: Sender<FrameId>,
}



impl BufferPoolManager {
    pub fn new(num_frames: usize, k_dist: usize) -> Self {
        let frames = (0..num_frames)
            .into_iter()
            .map(|it| Arc::new(RwLock::new(FrameHeader::new(it))))
            .collect();

        let internal = Arc::new(Mutex::new(BufferPoolInternal {
            frames,
            page_table: HashMap::with_capacity(num_frames as usize),
            replacer: LruKReplace::new(num_frames, k_dist),
            free_list: (0..num_frames as usize).collect(),
            frame_pin_count: (0..num_frames).into_iter().map(|_| AtomicU32::new(0)).collect(),
        }));

        let (tx, rx) = mpsc::channel();
        thread::spawn({
            let internal = internal.clone();
            move || {
            for frame_id in rx {
                let internal = internal.lock().unwrap();
                let pin_count: &AtomicU32 = internal.frame_pin_count.get(frame_id).unwrap();
                pin_count.fetch_sub(1, Ordering::SeqCst);
            }
        }});

        BufferPoolManager {
            num_frames,
            internal: internal,
            disk_scheduler: DiskScheduler::new(),
            next_page_id: AtomicUsize::new(0),
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
    pub fn delete_page(&self, page_id: PageId) -> bool {
        todo!()
    }

   
    fn read_page(&self, page_id: PageId) -> Option<FrameHeaderReadGuard> {
        let mut internal = self.internal.lock().unwrap();
        if let Some(frame_id) = internal.get_frame_id(page_id, &self.disk_scheduler) {
            let frame = internal.frames[frame_id].clone();
            let _guard = frame.read().unwrap();
            return Some(FrameHeaderReadGuard::new(frame.clone()));
        }
        None
    }

    fn write_page(&self, page_id: PageId) -> Option<FrameHeaderWriteGuard> {
        let mut internal = self.internal.lock().unwrap();
        if let Some(frame_id) = internal.get_frame_id(page_id, &self.disk_scheduler) {
            let frame = internal.frames[frame_id].clone();
            let mut guard = frame.write().unwrap();
            guard.dirty = true;
            return Some(FrameHeaderWriteGuard::new(frame.clone()));
        }

        None
    }
}


#[cfg(test)]
mod test {
   


    use super::BufferPoolManager;

    const FRAMES: usize = 10;
    const K_DIST: usize = 5;

    #[test]
    fn test_very_basic() {  
        //let disk_manager = MemoryManager::new(1000);
        let bpm = BufferPoolManager::new(FRAMES, K_DIST);
        let pid = bpm.new_page_id();
        let hello_world = "hello world";

        // Check `WritePageGuard` basic functionality.
        {
            let mut guard = bpm.write_page(pid).unwrap();
            let data = guard.get_write_guard().get_writeable_data();

            data[..hello_world.len()].copy_from_slice(hello_world.as_bytes());
        }

        // Check `ReadPageGuard` basic functionality.
        {
            assert_eq!(0, bpm.get_pin_count(pid).unwrap());
            let guard = bpm.read_page(pid).unwrap();
            let data = guard.get_read_guard().get_readable_data();

            assert_eq!(true, data[..hello_world.len()].eq(hello_world.as_bytes()));
        }

        // Check `ReadPageGuard` basic functionality (again).
        {
            assert_eq!(0, bpm.get_pin_count(pid).unwrap());
            let guard = bpm.read_page(pid).unwrap();
            let data = guard.get_read_guard().get_readable_data();

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
            let data = page_0_guard.get_write_guard().get_writeable_data();
            data[..page_0_data.len()].copy_from_slice(page_0_data.as_bytes());

            page_id_1 = bpm.new_page_id();
            let mut page_1_guard = bpm.write_page(page_id_1).unwrap();
            let data = page_1_guard.get_write_guard().get_writeable_data();
            data[..page_1_data.len()].copy_from_slice(page_1_data.as_bytes());

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
            let data = page_0_guard.get_write_guard().get_writeable_data();
            assert_eq!(true, data[..page_0_data.len()].eq(page_0_data.as_bytes()));
            data[..page_0_updated.len()].copy_from_slice(page_0_updated.as_bytes());

            let mut page_1_guard = bpm.write_page(page_id_1).unwrap();
            let data = page_1_guard.get_write_guard().get_writeable_data();
            assert_eq!(true, data[..page_1_data.len()].eq(page_1_data.as_bytes()));
            data[..page_1_updated.len()].copy_from_slice(page_1_updated.as_bytes());

            assert_eq!(1, bpm.get_pin_count(page_id_0).unwrap());
            assert_eq!(1, bpm.get_pin_count(page_id_1).unwrap());
        }

        assert_eq!(0, bpm.get_pin_count(page_id_0).unwrap());
        assert_eq!(0, bpm.get_pin_count(page_id_1).unwrap());

        {
            let page_0_guard = bpm.read_page(page_id_0).unwrap();
            let data = page_0_guard.get_read_guard().get_readable_data();
            assert_eq!(
                true,
                data[..page_0_updated.len()].eq(page_0_updated.as_bytes())
            );

            let page_1_guard = bpm.read_page(page_id_1).unwrap();
            let data = page_1_guard.get_read_guard().get_readable_data();
            assert_eq!(
                true,
                data[..page_1_updated.len()].eq(page_1_updated.as_bytes())
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
        let data = page_0_guard.get_write_guard().get_writeable_data();
        data[..hello.len()].copy_from_slice(hello.as_bytes());
        drop(page_0_guard);

        // Create a vector of unique pointers to page guards, which prevents the guards from getting destructed.
        let mut page_guards = Vec::new();

        // Scenario: We should be able to create new pages until we fill up the buffer pool.
        for i in 0..FRAMES {
            let page_id = bpm.new_page_id();
            let page_guard = bpm.write_page(page_id).unwrap();
            page_guards.push((page_id, page_guard));
        }

        // Scenario: All of the pin counts should be 1.
        for id_guard in page_guards.iter() {
            assert_eq!(1, bpm.get_pin_count(id_guard.0).unwrap());
        }

        // Scenario: Once the buffer pool is full, we should not be able to create any new pages.
        for i in 0..FRAMES {
            let page_id = bpm.new_page_id();
            let page_guard = bpm.write_page(page_id);
            assert_eq!(true, page_guard.is_none());
        }

        // Scenario: Drop the last 5 pages to unpin them.
        for i in 0..FRAMES / 2 {
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
        for i in 0..(FRAMES / 2) - 1 {
            let page_id = bpm.new_page_id();
            let page_guard = bpm.write_page(page_id);
            assert_eq!(true, page_guard.is_some());
        }

        // Scenario: There should be one frame available, and we should be able to fetch the data we wrote a while ago.
        {
            let original_page = bpm.read_page(page_0).unwrap();
            let data = original_page.get_read_guard().get_readable_data();
            assert_eq!(true, data[..hello.len()].eq(hello.as_bytes()));
        }

        // Scenario: Once we unpin page 0 and then make a new page, all the buffer pages should now be pinned. Fetching page 0
        // again should fail.
        let last_pid = bpm.new_page_id();
        let last_page = bpm.read_page(last_pid).unwrap();
        let last_pid = bpm.new_page_id();
        let last_page = bpm.read_page(last_pid).unwrap();
        let last_pid = bpm.new_page_id();
        let last_page = bpm.read_page(last_pid).unwrap();
        let last_pid = bpm.new_page_id();
        let last_page = bpm.read_page(last_pid).unwrap();

        let fail = bpm.read_page(page_0);
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
            let handler = s.spawn(|| {
                // The writer can keep writing to the same page.
                for i in 0..50 {
                    println!(
                        "Writer thread {:?} loop count {}",
                        thread::current().id(),
                        i
                    );
                    thread::sleep(Duration::from_millis(5));
                    let mut guard = bpm.write_page(pid).unwrap();
                    let to_write = i.to_string();
                    let data = guard.get_write_guard().get_writeable_data();
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
                    let cloned_data = String::from_utf8(Vec::from(
                        guard.get_read_guard().get_readable_data().clone(),
                    ))
                    .unwrap();

                    // Sleep for a bit. If latching is working properly, nothing should be writing to the page.
                    thread::sleep(Duration::from_millis(10));
                    let cloned_data_again = String::from_utf8(Vec::from(
                        guard.get_read_guard().get_readable_data().clone(),
                    ))
                    .unwrap();
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
                let guard = bpm.write_page(page_id_0);
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

    //     let disk_manager = MemoryManager::new(1000);
    //     let bpm = BufferPoolManager::new(1, K_DIST, Box::new(disk_manager));

    //     thread::scope(|s| {
    //         for i in 0..rounds {
    //             let mutex = Mutex::new(());
    //             let cond = Condvar::new();

    //             let mut signal = false;
    //             let winner_pid = bpm.new_page_id();
    //             let looser_pid = bpm.new_page_id();

    //             for j in 0..num_threads {
    //                 s.spawn(|| {
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
