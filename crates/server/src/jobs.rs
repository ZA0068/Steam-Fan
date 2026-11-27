//! Long work (downloads, unpacking, reading the shelf, checksums) runs on its own
//! thread. Clients see it through `GET /api/jobs` and can stop it.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use steam_fan_core::transfer::{Cancel, Progress};

#[derive(Debug, Clone, Serialize, Default)]
pub struct JobView {
    pub id: u64,
    pub title: String,
    /// The game it works on, if any: one job per game at a time.
    pub game: Option<String>,
    pub status: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub fraction: Option<f32>,
    pub done: bool,
    pub error: Option<String>,
    pub result: Option<String>,
    /// Seconds since 1970.
    pub started: u64,
    pub finished: Option<u64>,
}

#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    list: Arc<Mutex<Vec<JobView>>>,
    cancels: Arc<Mutex<HashMap<u64, Cancel>>>,
}

/// Handed to the work: report progress, check for Stop.
#[derive(Clone)]
pub struct Ctx {
    id: u64,
    list: Arc<Mutex<Vec<JobView>>>,
    pub cancel: Cancel,
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Ctx {
    fn update(&self, f: impl FnOnce(&mut JobView)) {
        if let Some(j) = self.list.lock().unwrap().iter_mut().find(|j| j.id == self.id) {
            f(j);
        }
    }
    pub fn status(&self, s: &str) {
        let s = s.to_string();
        self.update(|j| j.status = s);
    }
    pub fn progress(&self, p: &Progress) {
        let (d, t, f, cur) = (p.done_bytes, p.total_bytes, p.fraction(), p.current.clone());
        self.update(|j| {
            j.done_bytes = d;
            j.total_bytes = t;
            j.fraction = Some(f);
            if !cur.is_empty() {
                j.status = cur;
            }
        });
    }
}

impl Jobs {
    /// Start work on its own thread. Refused if a job for the same game is still running.
    pub fn spawn(
        &self,
        title: &str,
        game: Option<&str>,
        work: impl FnOnce(Ctx) -> anyhow::Result<String> + Send + 'static,
    ) -> anyhow::Result<u64> {
        let mut list = self.list.lock().unwrap();
        if let Some(g) = game {
            if let Some(j) = list.iter().find(|j| !j.done && j.game.as_deref() == Some(g)) {
                anyhow::bail!("busy with this game already: {}", j.title);
            }
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        list.push(JobView { id, title: title.into(), game: game.map(String::from), status: "starting".into(), started: now(), ..Default::default() });
        // keep the 50 newest finished jobs
        let finished: Vec<u64> = list.iter().filter(|j| j.done).map(|j| j.id).collect();
        if finished.len() > 50 {
            let drop: Vec<u64> = finished[..finished.len() - 50].to_vec();
            list.retain(|j| !drop.contains(&j.id));
        }
        drop(list);
        let cancel = Cancel::default();
        self.cancels.lock().unwrap().insert(id, cancel.clone());
        let ctx = Ctx { id, list: self.list.clone(), cancel };
        let cancels = self.cancels.clone();
        let title = title.to_string();
        eprintln!("[job {id}] {title}");
        std::thread::spawn(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(ctx.clone())))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("the job crashed")));
            let stopped = ctx.cancel.is_cancelled();
            match &res {
                Ok(m) => eprintln!("[job {id}] done: {m}"),
                Err(e) => eprintln!("[job {id}] failed: {e:#}"),
            }
            ctx.update(|j| {
                j.done = true;
                j.finished = Some(now());
                match res {
                    Ok(m) => {
                        j.result = Some(m);
                        j.status = "done".into();
                        j.fraction = Some(1.0);
                    }
                    Err(e) => {
                        j.error = Some(if stopped { "stopped".into() } else { format!("{e:#}") });
                        j.status = "failed".into();
                    }
                }
            });
            cancels.lock().unwrap().remove(&id);
        });
        Ok(id)
    }

    pub fn list(&self) -> Vec<JobView> {
        self.list.lock().unwrap().clone()
    }

    pub fn get(&self, id: u64) -> Option<JobView> {
        self.list.lock().unwrap().iter().find(|j| j.id == id).cloned()
    }

    pub fn running(&self, title_prefix: &str) -> bool {
        self.list.lock().unwrap().iter().any(|j| !j.done && j.title.starts_with(title_prefix))
    }

    pub fn cancel(&self, id: u64) -> bool {
        match self.cancels.lock().unwrap().get(&id) {
            Some(c) => {
                c.cancel();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_job_per_game_and_results() {
        let jobs = Jobs::default();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let a = jobs
            .spawn("A", Some("fan:x"), move |ctx| {
                ctx.status("working");
                rx.recv().ok();
                Ok("fine".into())
            })
            .unwrap();
        assert!(jobs.spawn("B", Some("fan:x"), |_| Ok(String::new())).is_err());
        tx.send(()).unwrap();
        for _ in 0..200 {
            if jobs.get(a).unwrap().done {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let j = jobs.get(a).unwrap();
        assert!(j.done);
        assert_eq!(j.result.as_deref(), Some("fine"));
        assert!(jobs.spawn("B", Some("fan:x"), |_| anyhow::bail!("nope")).is_ok());
    }
}
