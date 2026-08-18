//! Non-blocking decode mailbox.
//!
//! Capture publishes Finals into an unbounded FIFO: publishing never waits for
//! decoder capacity and Finals are never coalesced. Partials use one bounded
//! latest-wins slot. The decoder always checks the Final FIFO first.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError};

pub(crate) struct PartialJob {
    pub utterance_id: u64,
    pub pcm: Vec<f32>,
    pub utterance_start_ms: u64,
    pub pcm_start_ms: u64,
    pub pcm_end_ms: u64,
    pub queued_at: Instant,
}

pub(crate) struct FinalJob {
    pub utterance_id: u64,
    pub pcm: Vec<f32>,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub queued_at: Instant,
}

pub(crate) enum DecodeJob {
    Partial(PartialJob),
    Final(FinalJob),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartialPublish {
    Queued,
    Replaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MailboxClosed;

pub(crate) struct JobMailboxTx {
    final_tx: Sender<FinalJob>,
    partial_tx: Sender<PartialJob>,
    // A producer-side receiver makes replacing the single stale Partial a
    // bounded, non-blocking operation. JobMailboxTx intentionally is not Clone:
    // the Gate is the mailbox's only producer.
    partial_evict_rx: Receiver<PartialJob>,
    worker_alive: Arc<AtomicBool>,
}

pub(crate) struct JobMailboxRx {
    final_rx: Receiver<FinalJob>,
    partial_rx: Receiver<PartialJob>,
    worker_alive: Arc<AtomicBool>,
    deferred_partial: Option<PartialJob>,
}

pub(crate) fn job_mailbox() -> (JobMailboxTx, JobMailboxRx) {
    let (final_tx, final_rx) = crossbeam_channel::unbounded();
    let (partial_tx, partial_rx) = crossbeam_channel::bounded(1);
    let worker_alive = Arc::new(AtomicBool::new(true));
    (
        JobMailboxTx {
            final_tx,
            partial_tx,
            partial_evict_rx: partial_rx.clone(),
            worker_alive: worker_alive.clone(),
        },
        JobMailboxRx {
            final_rx,
            partial_rx,
            worker_alive,
            deferred_partial: None,
        },
    )
}

impl JobMailboxTx {
    pub fn publish_partial(&self, job: PartialJob) -> Result<PartialPublish, MailboxClosed> {
        if !self.worker_alive.load(Ordering::Acquire) {
            return Err(MailboxClosed);
        }
        match self.partial_tx.try_send(job) {
            Ok(()) => Ok(PartialPublish::Queued),
            Err(TrySendError::Disconnected(_)) => Err(MailboxClosed),
            Err(TrySendError::Full(job)) => {
                // A Full result proves there was one stale value. Either this
                // receiver removes it, or the worker wins the race and already
                // removed it; in both cases the second send has capacity.
                let evicted = match self.partial_evict_rx.try_recv() {
                    Ok(_) => true,
                    Err(TryRecvError::Empty) => false,
                    Err(TryRecvError::Disconnected) => return Err(MailboxClosed),
                };
                match self.partial_tx.try_send(job) {
                    Ok(()) if evicted => Ok(PartialPublish::Replaced),
                    Ok(()) => Ok(PartialPublish::Queued),
                    Err(TrySendError::Disconnected(_)) => Err(MailboxClosed),
                    // JobMailboxTx has one owner/producer, so nobody can refill
                    // the slot between the eviction and this retry.
                    Err(TrySendError::Full(_)) => unreachable!("single-producer partial slot"),
                }
            }
        }
    }

    pub fn publish_final(&self, job: FinalJob) -> Result<(), MailboxClosed> {
        if !self.worker_alive.load(Ordering::Acquire) || self.final_tx.send(job).is_err() {
            return Err(MailboxClosed);
        }
        Ok(())
    }

    pub fn final_depth(&self) -> usize {
        self.final_tx.len()
    }
}

impl JobMailboxRx {
    pub fn recv_next(&mut self) -> Result<DecodeJob, MailboxClosed> {
        if let Ok(final_job) = self.final_rx.try_recv() {
            return Ok(self.finish_final(final_job));
        }

        if let Some(partial) = self.deferred_partial.take() {
            let partial = match self.partial_rx.try_recv() {
                Ok(slot) if Self::partial_key(&slot) > Self::partial_key(&partial) => slot,
                Ok(_) | Err(TryRecvError::Empty) => partial,
                Err(TryRecvError::Disconnected) => return Err(MailboxClosed),
            };
            // Re-check after taking the deferred value so a concurrently
            // published Final still wins before decode begins.
            if let Ok(final_job) = self.final_rx.try_recv() {
                self.keep_future_partial(partial, final_job.utterance_id);
                return Ok(self.finish_final(final_job));
            }
            return Ok(DecodeJob::Partial(partial));
        }

        crossbeam_channel::select_biased! {
            recv(self.final_rx) -> result => {
                result
                    .map(|job| self.finish_final(job))
                    .map_err(|_| MailboxClosed)
            }
            recv(self.partial_rx) -> result => {
                let partial = result.map_err(|_| MailboxClosed)?;
                // A Final may have arrived between select choosing the
                // Partial and this thread resuming. Give it one last chance.
                if let Ok(final_job) = self.final_rx.try_recv() {
                    self.keep_future_partial(partial, final_job.utterance_id);
                    return Ok(self.finish_final(final_job));
                }
                Ok(DecodeJob::Partial(partial))
            }
        }
    }

    fn finish_final(&mut self, final_job: FinalJob) -> DecodeJob {
        let final_id = final_job.utterance_id;

        if let Some(partial) = self.deferred_partial.take() {
            self.keep_future_partial(partial, final_id);
        }
        // The slot has capacity one. Drop a same/older Partial; preserve a
        // Partial from a newer utterance until all ready Finals are processed.
        if let Ok(partial) = self.partial_rx.try_recv() {
            self.keep_future_partial(partial, final_id);
        }
        DecodeJob::Final(final_job)
    }

    fn partial_key(job: &PartialJob) -> (u64, u64) {
        (job.utterance_id, job.pcm_end_ms)
    }

    fn keep_future_partial(&mut self, candidate: PartialJob, final_id: u64) {
        if candidate.utterance_id <= final_id {
            return;
        }
        let replace = self
            .deferred_partial
            .as_ref()
            .map(|current| Self::partial_key(&candidate) > Self::partial_key(current))
            .unwrap_or(true);
        if replace {
            self.deferred_partial = Some(candidate);
        }
    }
}

impl Drop for JobMailboxRx {
    fn drop(&mut self) {
        self.worker_alive.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn partial(id: u64, marker: usize) -> PartialJob {
        PartialJob {
            utterance_id: id,
            pcm: vec![id as f32; marker],
            utterance_start_ms: id * 1_000,
            pcm_start_ms: id * 1_000,
            pcm_end_ms: id * 1_000 + marker as u64,
            queued_at: Instant::now(),
        }
    }

    fn final_job(id: u64) -> FinalJob {
        FinalJob {
            utterance_id: id,
            pcm: vec![id as f32; 4],
            t_start_ms: id * 1_000,
            t_end_ms: id * 1_000 + 500,
            queued_at: Instant::now(),
        }
    }

    #[test]
    fn latest_partial_wins() {
        let (tx, mut rx) = job_mailbox();
        assert_eq!(
            tx.publish_partial(partial(1, 1)),
            Ok(PartialPublish::Queued)
        );
        assert_eq!(
            tx.publish_partial(partial(1, 2)),
            Ok(PartialPublish::Replaced)
        );
        assert_eq!(
            tx.publish_partial(partial(1, 3)),
            Ok(PartialPublish::Replaced)
        );
        let DecodeJob::Partial(job) = rx.recv_next().unwrap() else {
            panic!("expected partial");
        };
        assert_eq!(job.pcm.len(), 3);
    }

    #[test]
    fn finals_are_fifo() {
        let (tx, mut rx) = job_mailbox();
        for id in 1..=3 {
            tx.publish_final(final_job(id)).unwrap();
        }
        assert_eq!(tx.final_depth(), 3);
        for id in 1..=3 {
            let DecodeJob::Final(job) = rx.recv_next().unwrap() else {
                panic!("expected final");
            };
            assert_eq!(job.utterance_id, id);
        }
        assert_eq!(tx.final_depth(), 0);
    }

    #[test]
    fn ready_final_has_priority_and_preserves_future_partial() {
        let (tx, mut rx) = job_mailbox();
        tx.publish_partial(partial(2, 2)).unwrap();
        tx.publish_final(final_job(1)).unwrap();
        let DecodeJob::Final(first) = rx.recv_next().unwrap() else {
            panic!("expected final first");
        };
        assert_eq!(first.utterance_id, 1);
        let DecodeJob::Partial(second) = rx.recv_next().unwrap() else {
            panic!("expected future partial second");
        };
        assert_eq!(second.utterance_id, 2);
    }

    #[test]
    fn newer_slot_replaces_deferred_partial() {
        let (tx, mut rx) = job_mailbox();
        tx.publish_partial(partial(2, 2)).unwrap();
        tx.publish_final(final_job(1)).unwrap();
        let DecodeJob::Final(_) = rx.recv_next().unwrap() else {
            panic!("expected final");
        };

        tx.publish_partial(partial(3, 3)).unwrap();
        let DecodeJob::Partial(job) = rx.recv_next().unwrap() else {
            panic!("expected partial");
        };
        assert_eq!(job.utterance_id, 3);
    }

    #[test]
    fn final_purges_same_utterance_partial() {
        let (tx, mut rx) = job_mailbox();
        tx.publish_partial(partial(1, 1)).unwrap();
        tx.publish_final(final_job(1)).unwrap();
        tx.publish_partial(partial(2, 2)).unwrap();
        let DecodeJob::Final(job) = rx.recv_next().unwrap() else {
            panic!("expected final");
        };
        assert_eq!(job.utterance_id, 1);
        let DecodeJob::Partial(job) = rx.recv_next().unwrap() else {
            panic!("expected newer partial");
        };
        assert_eq!(job.utterance_id, 2);
    }

    #[test]
    fn more_than_four_finals_publish_without_waiting_and_stay_ordered() {
        let (tx, mut rx) = job_mailbox();
        let started = Instant::now();
        for id in 0..32 {
            tx.publish_final(final_job(id)).unwrap();
        }
        assert!(started.elapsed() < Duration::from_millis(500));
        for id in 0..32 {
            let DecodeJob::Final(job) = rx.recv_next().unwrap() else {
                panic!("expected final");
            };
            assert_eq!(job.utterance_id, id);
        }
    }

    #[test]
    fn concurrent_receiver_never_loses_or_reorders_finals() {
        const COUNT: u64 = 128;
        let (tx, mut rx) = job_mailbox();
        let worker = std::thread::spawn(move || {
            let mut finals = Vec::with_capacity(COUNT as usize);
            while finals.len() < COUNT as usize {
                match rx.recv_next().unwrap() {
                    DecodeJob::Partial(_) => {}
                    DecodeJob::Final(job) => finals.push(job.utterance_id),
                }
            }
            finals
        });

        for id in 1..=COUNT {
            tx.publish_partial(partial(id, id as usize)).unwrap();
            tx.publish_final(final_job(id)).unwrap();
        }

        assert_eq!(worker.join().unwrap(), (1..=COUNT).collect::<Vec<_>>());
        assert_eq!(tx.final_depth(), 0);
    }

    #[test]
    fn final_pcm_allocation_is_moved() {
        let (tx, mut rx) = job_mailbox();
        let job = final_job(1);
        let ptr = job.pcm.as_ptr();
        tx.publish_final(job).unwrap();
        let DecodeJob::Final(job) = rx.recv_next().unwrap() else {
            panic!("expected final");
        };
        assert_eq!(job.pcm.as_ptr(), ptr);
    }

    #[test]
    fn receiver_drop_makes_final_publish_fail() {
        let (tx, rx) = job_mailbox();
        drop(rx);
        assert_eq!(tx.publish_final(final_job(1)), Err(MailboxClosed));
    }

    #[test]
    fn receiver_drop_makes_partial_publish_fail() {
        let (tx, rx) = job_mailbox();
        drop(rx);
        assert_eq!(tx.publish_partial(partial(1, 1)), Err(MailboxClosed));
    }
}
