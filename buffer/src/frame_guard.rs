#![allow(dead_code)]

use std::{
    ops::{Deref, DerefMut},
    sync::{mpsc::Sender, Arc},
};

use common::{FrameId, PAGE_SIZE};
use tokio::sync::oneshot;

use crate::buffer_pool_manager::FrameHeader;
pub(crate) struct FrameHeaderReadGuard {
    pub(crate) frame: Arc<FrameHeader>,
    pub(crate) pin_reducer: Sender<FrameGuardDropMessage>,
}

pub(crate) struct FrameHeaderWriteGuard {
    pub(crate) frame: Option<FrameHeader>,
    pub(crate) pin_reducer: Sender<FrameGuardDropMessage>,
}

impl Deref for FrameHeaderReadGuard {
    type Target = [u8; PAGE_SIZE];

    fn deref(&self) -> &Self::Target {
        self.frame.get_readable()
    }
}

/// We know frame will always have value till the time it get dropped.
impl Deref for FrameHeaderWriteGuard {
    type Target = FrameHeader;

    fn deref(&self) -> &Self::Target {
        self.frame.as_ref().unwrap()
    }
}

/// We know frame will always have value till the time it get dropped.
impl DerefMut for FrameHeaderWriteGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.frame.as_mut().unwrap()
    }
}

/// Implementation choice -
/// ###parameter - tx
/// it is not needed. But the way test is written in original C++, it expects things to happen linear.
/// Needed to have channel for linearizability across channel.
pub(crate) enum FrameGuardDropMessage {
    Read {
        frame_id: FrameId,
        tx: oneshot::Sender<()>,
    },
    Write {
        guard: FrameHeader,
        tx: oneshot::Sender<()>,
    },
}

impl FrameHeaderReadGuard {
    fn new(frame: Arc<FrameHeader>, pin_reducer: Sender<FrameGuardDropMessage>) -> Self {
        FrameHeaderReadGuard { frame, pin_reducer }
    }
}

impl FrameHeaderWriteGuard {
    fn new(frame: FrameHeader, pin_reducer: Sender<FrameGuardDropMessage>) -> Self {
        FrameHeaderWriteGuard {
            frame: Some(frame),
            pin_reducer,
        }
    }
}

impl Drop for FrameHeaderReadGuard {
    fn drop(&mut self) {
        let (tx, rx) = oneshot::channel();
        let _ = self.pin_reducer.send(FrameGuardDropMessage::Read {
            frame_id: self.frame.get_frame_id(),
            tx,
        });

        let _ = rx.blocking_recv();
    }
}

impl Drop for FrameHeaderWriteGuard {
    fn drop(&mut self) {
        let (tx, rx) = oneshot::channel();
        let _ = self.pin_reducer.send(FrameGuardDropMessage::Write {
            guard: self.frame.take().unwrap(),
            tx,
        });

        let _ = rx.blocking_recv();
    }
}
