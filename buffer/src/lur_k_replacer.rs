#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Instant,
};

use common::FrameId;

type ElapsedTime = u128;
pub enum AccessType {
    Unknown = 0,
    Lookup,
    Scan,
    Index,
}

static START_TIME: LazyLock<Instant> = LazyLock::new(|| Instant::now());
struct LruNode {
    history: Vec<ElapsedTime>,
    frame_id: FrameId,
    history_size: usize,
    evictable: bool,
}

impl LruNode {
    fn new(frame_id: FrameId, history_size: usize) -> Self {
        LruNode {
            history: Vec::with_capacity(history_size),
            frame_id,
            history_size,
            evictable: false,
        }
    }

    fn add_access(&mut self) {
        if self.history.len() >= self.history_size {
            self.history.pop();
        }

        let elapsed_time = START_TIME.elapsed().as_nanos();
        self.history.push(elapsed_time);
    }
}

///LRUKReplacer implements the LRU-k replacement policy.
///
/// The LRU-k algorithm evicts a frame whose backward k-distance is maximum
/// of all frames. Backward k-distance is computed as the difference in time between
/// current timestamp and the timestamp of kth previous access.
///
/// A frame with less than k historical references is given
/// +inf as its backward k-distance. When multiple frames have +inf backward k-distance,
/// classical LRU algorithm is used to choose victim.
pub struct LruKReplace {
    max_frame: u32,
    history_size: usize,
    nodes: Mutex<HashMap<FrameId, LruNode>>,
}

impl LruKReplace {
    pub fn new(max_frame: u32, k_size: usize) -> Self {
        LruKReplace {
            max_frame,
            history_size: k_size,
            nodes: Mutex::new(HashMap::with_capacity(max_frame as usize)),
        }
    }

    /// Remove an evictable frame from replacer, along with its access history.
    /// This function should also decrement replacer's size if removal is successful.
    /// Note that this is different from evicting a frame, which always remove the frame
    ///  with largest backward k-distance. This function removes specified frame id, no matter what its backward k-distance is.
    ///
    /// If Remove is called on a non-evictable frame, throw an exception or abort the process.
    ///
    /// If specified frame is not found, directly return from this function.
    pub fn remove(&self, frame_id: FrameId) {
        let mut guard = self.nodes.lock().unwrap();
        if let Some(node) = guard.get(&frame_id) {
            if !node.evictable {
                panic!("Remove is called on non evictable frame {}", frame_id);
            }
        }
        guard.remove(&frame_id);
    }

    ///Find the frame with largest backward k-distance and evict that frame. Only frames
    /// that are marked as 'evictable' are candidates for eviction.
    ///
    /// A frame with less than k historical references is given +inf as its backward k-distance.
    /// If multiple frames have inf backward k-distance, then evict frame with earliest timestamp
    /// based on LRU.
    ///
    /// Successful eviction of a frame should decrement the size of replacer and remove the frame's access history.
    pub fn evict(&self) -> Option<FrameId> {
        let mut guard = self.nodes.lock().unwrap();
        // Frame Id to be removed, past accessed time, if has k entries or not
        let mut to_be_evicted_frame: Option<(FrameId, ElapsedTime, bool)> = None;
        let mut found_less_than_k_entry = false;
        for (_, node) in guard.iter_mut() {
            if !node.evictable {
                continue;
            }

            if found_less_than_k_entry && node.history.len().eq(&self.history_size) {
                continue;
            }

            let existing_elapsed_time = to_be_evicted_frame
                .map(|it| it.1)
                .unwrap_or(ElapsedTime::MAX);
            let current_node_elapsed_time = node.history.last().cloned().unwrap_or(0);

            let mut has_less_than_k_entry = false;

            if node.history.len() < self.history_size {
                has_less_than_k_entry = true;
                if !found_less_than_k_entry {
                    found_less_than_k_entry = true;
                    to_be_evicted_frame = Some((
                        node.frame_id,
                        node.history.last().unwrap().clone(),
                        has_less_than_k_entry,
                    ));
                }
            }

            if current_node_elapsed_time < existing_elapsed_time {
                to_be_evicted_frame = Some((
                    node.frame_id,
                    current_node_elapsed_time,
                    has_less_than_k_entry,
                ))
            }
        }

        to_be_evicted_frame.map(|it| {
            guard.remove(&it.0);
            it.0
        })
    }

    /// If frame id is invalid, throw an exception or abort the process.
    /// Toggle whether a frame is evictable or non-evictable. This function also
    /// controls replacer's size. Note that size is equal to number of evictable entries.
    /// If a frame was previously evictable and is to be set to non-evictable, then size should
    /// decrement. If a frame was previously non-evictable and is to be set to evictable, then size should increment.
    ///
    ///  For other scenarios, this function should terminate without modifying anything.
    pub fn set_evictable(&self, frame_id: FrameId, is_evictable: bool) {
        self.panic_if_not_valid_frame_id(frame_id);

        let mut guard = self.nodes.lock().unwrap();
        let Some(node) = guard.get_mut(&frame_id) else {
            return;
        };

        if node.evictable && !is_evictable {
            node.evictable = false;
        } else if !node.evictable && is_evictable {
            node.evictable = true;
        }
    }

    pub fn record_access(&self, frame_id: FrameId) {
        self.record_access_with_type(frame_id, AccessType::Unknown);
    }

    /// Record the event that the given frame id is accessed at current timestamp.
    /// Create a new entry for access history if frame id has not been seen before.
    /// If frame id is invalid (ie. larger than replacer_size_), throw an exception.
    pub fn record_access_with_type(&self, frame_id: FrameId, _access_type: AccessType) {
        self.panic_if_not_valid_frame_id(frame_id);
        let mut nodes = self.nodes.lock().unwrap();
        if let Some(node) = nodes.get_mut(&frame_id) {
            node.add_access();
            return;
        }

        let mut node = LruNode::new(frame_id, self.history_size);
        node.add_access();

        nodes.insert(frame_id, node);
    }

    pub fn count_of_evictable(&self) -> u32 {
        let guard = self.nodes.lock().unwrap();
        guard.iter().filter(|it| it.1.evictable).count() as u32
    }

    fn panic_if_not_valid_frame_id(&self, frame_id: FrameId) {
        if self.max_frame < frame_id {
            panic!("Invalid frame id {}", frame_id);
        }
    }
}

#[cfg(test)]
mod test {
    use crate::LruKReplace;

    #[test]
    fn sample_test() {
        let replacer = LruKReplace::new(7, 2);
        for i in 1..7 {
            replacer.record_access(i);
        }

        // Add six frames to the replacer. We now have frames [1, 2, 3, 4, 5]. We set frame 6 as non-evictable.
        for i in 1..6 {
            replacer.set_evictable(i, true);
        }
        replacer.set_evictable(6, false);

        // The size of the replacer is the number of frames that can be evicted, _not_ the total number of frames entered.
        assert_eq!(5, replacer.count_of_evictable());

        // Record an access for frame 1. Now frame 1 has two accesses total.
        replacer.record_access(1);

        // Evict three pages from the replacer.
        // To break ties, we use LRU with respect to the oldest timestamp, or the least recently used frame.
        for i in 2..=4 {
            assert_eq!(i, replacer.evict().unwrap());
        }
        assert_eq!(2, replacer.count_of_evictable());

        // Now the replacer has the frames [5, 1].

        // Insert new frames [3, 4], and update the access history for 5. Now, the ordering is [3, 1, 5, 4].
        for i in [3, 4, 5, 4] {
            replacer.record_access(i);
        }
        replacer.set_evictable(3, true);
        replacer.set_evictable(4, true);
        assert_eq!(4, replacer.count_of_evictable());

        // Look for a frame to evict. We expect frame 3 to be evicted next.
        assert_eq!(3, replacer.evict().unwrap());
        assert_eq!(3, replacer.count_of_evictable());

        // Set 6 to be evictable. 6 Should be evicted next since it has the maximum backward k-distance.
        replacer.set_evictable(6, true);
        assert_eq!(4, replacer.count_of_evictable());
        assert_eq!(6, replacer.evict().unwrap());
        assert_eq!(3, replacer.count_of_evictable());

        // Mark frame 1 as non-evictable. We now have [5, 4].
        replacer.set_evictable(1, false);

        // We expect frame 5 to be evicted next.
        assert_eq!(2, replacer.count_of_evictable());
        assert_eq!(5, replacer.evict().unwrap());
        assert_eq!(1, replacer.count_of_evictable());

        // Update the access history for frame 1 and make it evictable. Now we have [4, 1].
        replacer.record_access(1);
        replacer.record_access(1);
        replacer.set_evictable(1, true);
        assert_eq!(2, replacer.count_of_evictable());

        // Evict the last two frames.
        assert_eq!(4, replacer.evict().unwrap());
        assert_eq!(1, replacer.count_of_evictable());
        assert_eq!(1, replacer.evict().unwrap());
        assert_eq!(0, replacer.count_of_evictable());

        // Insert frame 1 again and mark it as non-evictable.
        replacer.record_access(1);
        replacer.set_evictable(1, false);
        assert_eq!(0, replacer.count_of_evictable());

        // A failed eviction should not change the size of the replacer.
        assert_eq!(None, replacer.evict());

        // Mark frame 1 as evictable again and evict it.
        replacer.set_evictable(1, true);
        assert_eq!(1, replacer.count_of_evictable());
        assert_eq!(1, replacer.evict().unwrap());
        assert_eq!(0, replacer.count_of_evictable());

        // There is nothing left in the replacer, so make sure this doesn't do something strange.
        assert_eq!(None, replacer.evict());
        assert_eq!(0, replacer.count_of_evictable());

        // Make sure that setting a non-existent frame as evictable or non-evictable doesn't do something strange.
        replacer.set_evictable(6, false);
        replacer.set_evictable(6, true);
    }
}
