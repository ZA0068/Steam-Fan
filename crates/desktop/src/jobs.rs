//! Work that takes a while (copying, reading the shelf, artwork) runs on its own
//! thread. The window gets a `jobs` event whenever something changes, and a
//! `library-changed` event when the game list should be read again.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use steam_fan_core::transfer::{Cancel, Progress};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Default)]
pub struct JobView {
    pub id: u64,
    pub title: String,
    pub status: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub fraction: Option<f32>,
    pub done: bool,
    pub error: Option<String>,
    pub result: Option<String>,
}

#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    list: Arc<Mutex<Vec<JobView>>>,
    cancels: Mutex<Vec<(u64, Cancel)>>,
}

/// Handed to the work: report progress, check for Stop.
#[derive(Clone)]
pub struct Ctx {
    id: u64,
    list: Arc<Mutex<Vec<JobView>>>,
    handle: AppHandle,
    pub cancel: Cancel,
    last: Arc<Mutex<std::time::Instant>>,
}

impl Ctx {
    fn update(&self, f: impl FnOnce(&mut JobView), force: bool) {
        {
            let mut l = self.list.lock().unwrap();
            if let Some(j) = l.iter_mut().find(|j| j.id == self.id) {
                f(j);
            }
        }
        let mut last = self.last.lock().unwrap();
        if force || last.elapsed().as_millis() > 120 {
            *last = std::time::Instant::now();
            let _ = self.handle.emit("jobs", self.list.lock().unwrap().clone());
        }
    }
    pub fn status(&self, s: &str) {
        let s = s.to_string();
        self.update(|j| j.status = s, false);
    }
    pub fn progress(&self, p: &Progress) {
        let (d, t, frac) = (p.done_bytes, p.total_bytes, p.fraction());
        let cur = p.current.clone();
        self.update(
            |j| {
                j.done_bytes = d;
                j.total_bytes = t;
                j.fraction = Some(frac);
                j.status = cur;
            },
            false,
        );
    }
    /// The game list changed: the window reads it again.
    pub fn reload(&self) {
        let _ = self.handle.emit("library-changed", ());
    }
}

impl Jobs {
    pub fn spawn(&self, handle: &AppHandle, title: &str, work: impl FnOnce(Ctx) -> anyhow::Result<String> + Send + 'static) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Cancel::default();
        {
            let mut l = self.list.lock().unwrap();
            // forget finished jobs older than the last few
            let finished: Vec<u64> = l.iter().filter(|j| j.done).map(|j| j.id).collect();
            if finished.len() > 8 {
                let drop: Vec<u64> = finished[..finished.len() - 8].to_vec();
                l.retain(|j| !drop.contains(&j.id));
            }
            l.push(JobView { id, title: title.to_string(), ..Default::default() });
        }
        self.cancels.lock().unwrap().push((id, cancel.clone()));
        let ctx = Ctx {
            id,
            list: self.list.clone(),
            handle: handle.clone(),
            cancel,
            last: Arc::new(Mutex::new(std::time::Instant::now())),
        };
        let _ = handle.emit("jobs", self.list.lock().unwrap().clone());
        std::thread::spawn(move || {
            let r = work(ctx.clone());
            ctx.update(
                |j| {
                    j.done = true;
                    match r {
                        Ok(m) => j.result = Some(m),
                        Err(e) => j.error = Some(format!("{e:#}")),
                    }
                },
                true,
            );
            ctx.reload();
        });
        id
    }

    pub fn list(&self) -> Vec<JobView> {
        self.list.lock().unwrap().clone()
    }

    pub fn cancel(&self, id: u64) {
        if let Some((_, c)) = self.cancels.lock().unwrap().iter().find(|(i, _)| *i == id) {
            c.cancel();
        }
    }
}
