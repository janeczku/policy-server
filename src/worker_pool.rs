use crate::communication::EvalRequest;
use anyhow::{anyhow, Result};
use policy_evaluator::policy::Policy;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    },
    thread,
    thread::JoinHandle,
    vec::Vec,
};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing::{debug, error, info};

use crate::worker::Worker;

pub(crate) struct WorkerPool {
    pool_size: usize,
    worker_tx_chans: Vec<Sender<EvalRequest>>,
    api_rx: Receiver<EvalRequest>,
    join_handles: Vec<JoinHandle<Result<()>>>,
}

impl WorkerPool {
    #[tracing::instrument]
    pub(crate) fn new(
        size: usize,
        policies: HashMap<String, Policy>,
        rx: Receiver<EvalRequest>,
        barrier: Arc<Barrier>,
        boot_canary: Arc<AtomicBool>,
    ) -> WorkerPool {
        let mut tx_chans = Vec::<Sender<EvalRequest>>::new();
        let mut handles = Vec::<JoinHandle<Result<()>>>::new();

        for n in 1..=size {
            let (tx, rx) = channel::<EvalRequest>(32);
            tx_chans.push(tx);
            let ps = policies.clone();
            let b = barrier.clone();
            let canary = boot_canary.clone();

            let join = thread::spawn(move || -> Result<()> {
                info!(spawned = n, total = size, "spawning worker");
                let worker = match Worker::new(rx, ps) {
                    Ok(w) => w,
                    Err(e) => {
                        error!(error = e.to_string().as_str(), "cannot spawn worker");
                        canary.store(false, Ordering::SeqCst);
                        b.wait();
                        return Err(anyhow!("Worker {} couldn't start: {:?}", n, e));
                    }
                };
                b.wait();

                debug!(id = n, "worker loop start");
                worker.run();
                debug!(id = n, "worker loop exit");

                Ok(())
            });
            handles.push(join);
        }

        WorkerPool {
            pool_size: size,
            worker_tx_chans: tx_chans,
            api_rx: rx,
            join_handles: handles,
        }
    }

    pub(crate) fn run(mut self) {
        let mut next_worker_id = 0;

        while let Some(req) = self.api_rx.blocking_recv() {
            let _ = self.worker_tx_chans[next_worker_id].blocking_send(req);
            next_worker_id += 1;
            if next_worker_id >= self.pool_size {
                next_worker_id = 0;
            }
        }

        for handle in self.join_handles {
            handle.join().unwrap().unwrap();
        }
    }
}
