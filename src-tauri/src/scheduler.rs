// Yard — offline batch file compression
// Copyright (C) 2026 Yard Contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

pub struct LaneSpec<J> {
    pub key: String,
    pub jobs: VecDeque<J>,
}

pub struct LaneJob {
    pub file_id: String,
    pub estimate: u64,
}

pub async fn run_budgeted_lanes<J, F, Fut, S, C, E>(
    mut lanes: Vec<LaneSpec<J>>,
    budget: u64,
    should_stop: S,
    mut run_job: F,
    mut on_skipped: C,
    estimate: E,
) -> u64
where
    J: Send + 'static,
    F: FnMut(J) -> Fut + Send,
    Fut: Future<Output = u64> + Send + 'static,
    S: Fn() -> bool + Send + Sync,
    C: FnMut(&J) + Send,
    E: Fn(&J) -> u64 + Send + Sync,
{
    let mut used: u64 = 0;
    let mut total: u64 = 0;
    let mut inflight: Vec<(usize, u64, tokio::task::JoinHandle<u64>)> = Vec::new();
    let mut oversize_warned = false;

    loop {
        let mut dispatched = false;
        for idx in 0..lanes.len() {
            if lanes[idx].jobs.is_empty() {
                continue;
            }
            if inflight.iter().any(|(i, _, _)| *i == idx) {
                continue;
            }
            let job = lanes[idx].jobs.front().expect("checked non-empty");
            let est = estimate(job);
            let can_admit = if used + est <= budget {
                true
            } else if est > budget && inflight.is_empty() {
                if !oversize_warned {
                    oversize_warned = true;
                    eprintln!(
                        "warn: job estimate {est} exceeds memory budget {budget}; running oversize jobs alone to avoid deadlock"
                    );
                }
                true
            } else {
                false
            };
            if !can_admit {
                continue;
            }
            if should_stop() {
                for lane in lanes.iter_mut() {
                    while let Some(job) = lane.jobs.pop_front() {
                        on_skipped(&job);
                    }
                }
                break;
            }
            let job = lanes[idx].jobs.pop_front().expect("checked non-empty");
            used = used.saturating_add(est);
            let handle = tokio::spawn(run_job(job));
            inflight.push((idx, est, handle));
            dispatched = true;
            break;
        }

        if !dispatched {
            if inflight.is_empty() {
                break;
            }
            let (done, result) = std::future::poll_fn(|cx| {
                for (i, (_, _, handle)) in inflight.iter_mut().enumerate() {
                    if let Poll::Ready(result) = Pin::new(handle).poll(cx) {
                        return Poll::Ready((i, result));
                    }
                }
                Poll::Pending
            })
            .await;
            let (_idx, est, _) = inflight.remove(done);
            let output = result.unwrap_or_else(|e| {
                eprintln!("warn: lane job panicked, counting as 0: {e}");
                0
            });
            used = used.saturating_sub(est);
            total += output;
        }
    }

    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::future::Future;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Probe {
        active: Arc<Mutex<HashMap<String, usize>>>,
        max_seen: Arc<Mutex<HashMap<String, usize>>>,
        weight: Arc<Mutex<u64>>,
        max_weight: Arc<Mutex<u64>>,
        order: Arc<Mutex<Vec<String>>>,
    }

    impl Probe {
        fn new() -> Self {
            Probe {
                active: Arc::new(Mutex::new(HashMap::new())),
                max_seen: Arc::new(Mutex::new(HashMap::new())),
                weight: Arc::new(Mutex::new(0)),
                max_weight: Arc::new(Mutex::new(0)),
                order: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn enter(&self, lane: &str, job: &str, weight: u64) {
            self.order.lock().unwrap().push(job.to_string());
            let n = {
                let mut active = self.active.lock().unwrap();
                let n = *active.entry(lane.to_string()).or_insert(0) + 1;
                active.insert(lane.to_string(), n);
                n
            };
            let mut max = self.max_seen.lock().unwrap();
            let seen = *max.get(lane).unwrap_or(&0);
            max.insert(lane.to_string(), seen.max(n));
            drop(max);
            let mut w = self.weight.lock().unwrap();
            *w += weight;
            let mut max_w = self.max_weight.lock().unwrap();
            *max_w = (*max_w).max(*w);
        }

        fn exit(&self, lane: &str, weight: u64) {
            let mut active = self.active.lock().unwrap();
            let n = active.get(lane).copied().unwrap_or(1);
            if n <= 1 {
                active.remove(lane);
            } else {
                active.insert(lane.to_string(), n - 1);
            }
            drop(active);
            let mut w = self.weight.lock().unwrap();
            *w = w.saturating_sub(weight);
        }

        fn max_weight(&self) -> u64 {
            *self.max_weight.lock().unwrap()
        }
    }

    fn make_lanes(spec: &[(&str, u64, Vec<&str>)]) -> Vec<LaneSpec<String>> {
        spec.iter()
            .map(|(key, _estimate, jobs)| LaneSpec {
                key: key.to_string(),
                jobs: jobs.iter().map(|j| format!("{key}:{j}")).collect(),
            })
            .collect()
    }

    fn estimate(job: &String) -> u64 {
        if job.starts_with("video:") { 200 } else { 100 }
    }

    fn job_future(probe: &Probe, job: String, weight: u64) -> impl Future<Output = u64> {
        let probe = probe.clone();
        let (lane, _) = job.split_once(':').unwrap();
        let lane = lane.to_string();
        async move {
            probe.enter(&lane, &job, weight);
            for _ in 0..3 {
                tokio::task::yield_now().await;
            }
            probe.exit(&lane, weight);
            weight
        }
    }

    #[tokio::test]
    async fn same_category_never_runs_two_at_once() {
        let probe = Probe::new();
        let lanes = make_lanes(&[("image", 100, vec!["a", "b", "c"])]);
        let probe2 = probe.clone();
        let total = run_budgeted_lanes(lanes, 1000, || false, move |j| {
            let weight = estimate(&j);
            job_future(&probe2, j, weight)
        }, |_| {}, estimate).await;
        assert_eq!(total, 300);
        assert_eq!(*probe.max_seen.lock().unwrap().get("image").unwrap(), 1);
    }

    #[tokio::test]
    async fn lanes_dispatch_oldest_first_within_budget() {
        let probe = Probe::new();
        let lanes = make_lanes(&[("image", 100, vec!["a", "b"]), ("pdf", 100, vec!["p"])]);
        let probe2 = probe.clone();
        run_budgeted_lanes(lanes, 1000, || false, move |j| {
            let weight = estimate(&j);
            job_future(&probe2, j, weight)
        }, |_| {}, estimate).await;
        let order = probe.order.lock().unwrap().clone();
        assert_eq!(order, vec!["image:a", "pdf:p", "image:b"]);
    }

    #[tokio::test]
    async fn video_occupies_more_budget_image_plus_video_fill_budget() {
        let probe = Probe::new();
        let lanes = make_lanes(&[
            ("video", 200, vec!["v"]),
            ("image", 100, vec!["i1"]),
            ("image", 100, vec!["i2"]),
        ]);
        let probe2 = probe.clone();
        run_budgeted_lanes(lanes, 400, || false, move |j| {
            let weight = estimate(&j);
            job_future(&probe2, j, weight)
        }, |_| {}, estimate).await;
        assert_eq!(*probe.max_seen.lock().unwrap().get("video").unwrap(), 1);
        assert_eq!(*probe.max_seen.lock().unwrap().get("image").unwrap(), 2);
        assert_eq!(probe.max_weight(), 400);
    }

    #[tokio::test]
    async fn scheduler_waits_when_estimate_exceeds_budget() {
        let probe = Probe::new();
        let lanes = make_lanes(&[
            ("video", 200, vec!["v1"]),
            ("video", 200, vec!["v2"]),
        ]);
        let probe2 = probe.clone();
        let total = run_budgeted_lanes(lanes, 300, || false, move |j| {
            let weight = estimate(&j);
            job_future(&probe2, j, weight)
        }, |_| {}, estimate).await;
        assert_eq!(total, 400);
        assert_eq!(*probe.max_seen.lock().unwrap().get("video").unwrap(), 1);
    }

    #[tokio::test]
    async fn oversize_job_runs_alone_instead_of_deadlocking() {
        let lanes = make_lanes(&[("image", 100, vec!["big"])]);
        let total = run_budgeted_lanes(lanes, 50, || false, |j| async move {
            tokio::task::yield_now().await;
            estimate(&j)
        }, |_| {}, estimate).await;
        assert_eq!(total, 100);
    }

    #[tokio::test]
    async fn every_oversize_job_runs_alone_instead_of_deadlocking() {
        let probe = Probe::new();
        let lanes = make_lanes(&[
            ("video", 200, vec!["v1"]),
            ("video", 200, vec!["v2"]),
        ]);
        let probe2 = probe.clone();
        let total = run_budgeted_lanes(lanes, 50, || false, move |j| {
            let weight = estimate(&j);
            job_future(&probe2, j, weight)
        }, |_| {}, estimate).await;
        assert_eq!(total, 400);
        assert_eq!(*probe.max_seen.lock().unwrap().get("video").unwrap(), 1);
    }

    #[tokio::test]
    async fn stop_hook_skips_remaining_and_sums_completed() {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop2 = stop.clone();
        let skipped = Arc::new(Mutex::new(Vec::new()));
        let skipped2 = skipped.clone();
        let lanes = make_lanes(&[("image", 100, vec!["a", "b", "c"])]);
        let total = run_budgeted_lanes(
            lanes,
            200,
            move || stop2.load(std::sync::atomic::Ordering::SeqCst),
            |j| {
                let stop = stop.clone();
                async move {
                    stop.store(true, std::sync::atomic::Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    if j.ends_with('a') { 1 } else { 0 }
                }
            },
            move |j| skipped2.lock().unwrap().push(j.clone()),
            estimate,
        )
        .await;
        assert_eq!(total, 1);
        assert_eq!(*skipped.lock().unwrap(), vec!["image:b", "image:c"]);
    }

    #[tokio::test]
    async fn drains_to_empty_and_returns_sum() {
        let lanes = make_lanes(&[("image", 100, vec!["a", "b"]), ("pdf", 100, vec!["p", "q"])]);
        let total = run_budgeted_lanes(lanes, 200, || false, |j| async move {
            tokio::task::yield_now().await;
            if j.ends_with('a') || j.ends_with('p') { 3 } else { 4 }
        }, |_| {}, estimate).await;
        assert_eq!(total, 14);
    }

    #[tokio::test]
    async fn never_exceeds_budget() {
        let probe = Probe::new();
        let lanes = make_lanes(&[
            ("image", 100, vec!["a"]),
            ("pdf", 100, vec!["p"]),
            ("doc", 100, vec!["d"]),
            ("video", 200, vec!["v"]),
        ]);
        let probe2 = probe.clone();
        let total = run_budgeted_lanes(lanes, 200, || false, move |j| {
            let weight = estimate(&j);
            job_future(&probe2, j, weight)
        }, |_| {}, estimate).await;
        assert_eq!(total, 500);
        assert_eq!(probe.max_weight(), 200);
    }

    #[tokio::test]
    async fn panicking_job_does_not_kill_batch_or_sum() {
        let lanes = make_lanes(&[("image", 100, vec!["a"]), ("pdf", 100, vec!["p"])]);
        let total = run_budgeted_lanes(lanes, 200, || false, |j| async move {
            tokio::task::yield_now().await;
            if j.starts_with("image") {
                panic!("boom");
            }
            7
        }, |_| {}, estimate).await;
        assert_eq!(total, 7);
    }

    #[tokio::test]
    async fn stop_before_start_skips_everything() {
        let skipped = Arc::new(Mutex::new(Vec::new()));
        let skipped2 = skipped.clone();
        let lanes = make_lanes(&[("image", 100, vec!["a", "b"]), ("pdf", 100, vec!["p", "q"])]);
        let total = run_budgeted_lanes(lanes, 200, || true, |_j| async { 0u64 }, move |j| {
            skipped2.lock().unwrap().push(j.clone())
        }, estimate)
        .await;
        assert_eq!(total, 0);
        assert_eq!(
            *skipped.lock().unwrap(),
            vec!["image:a", "image:b", "pdf:p", "pdf:q"]
        );
    }

    #[tokio::test]
    async fn two_videos_do_not_overlap_even_with_large_budget() {
        let probe = Probe::new();
        let lanes = make_lanes(&[("video", 200, vec!["v1", "v2"])]);
        let probe2 = probe.clone();
        let total = run_budgeted_lanes(lanes, 1000, || false, move |j| {
            let weight = estimate(&j);
            job_future(&probe2, j, weight)
        }, |_| {}, estimate).await;
        assert_eq!(total, 400);
        assert_eq!(*probe.max_seen.lock().unwrap().get("video").unwrap(), 1);
    }

    #[tokio::test]
    async fn frees_budget_as_soon_as_jobs_finish() {
        let probe = Probe::new();
        let order = Arc::new(Mutex::new(Vec::new()));
        let lanes = make_lanes(&[
            ("video", 200, vec!["v"]),
            ("image", 100, vec!["i1", "i2"]),
        ]);
        let probe2 = probe.clone();
        let order2 = order.clone();
        run_budgeted_lanes(lanes, 300, || false, move |j| {
            let probe = probe2.clone();
            let order = order2.clone();
            let weight = estimate(&j);
            let iterations: usize = if j.starts_with("video") { 50 } else { 1 };
            async move {
                let lane = j.split_once(':').unwrap().0;
                probe.enter(lane, &j, weight);
                order.lock().unwrap().push(j.clone());
                for _ in 0..iterations {
                    tokio::task::yield_now().await;
                }
                probe.exit(lane, weight);
                order.lock().unwrap().push(format!("{j}:exit"));
                weight
            }
        }, |_| {}, estimate).await;

        let order = order.lock().unwrap().clone();
        let i2_enter = order.iter().position(|e| e == "image:i2");
        let v_exit = order.iter().position(|e| e == "video:v:exit");
        assert!(i2_enter.is_some(), "i2 must dispatch, log: {order:?}");
        assert!(v_exit.is_some(), "v must exit, log: {order:?}");
        assert!(
            i2_enter.unwrap() < v_exit.unwrap(),
            "queued image must dispatch while the long video still runs, log: {order:?}"
        );
    }
}
