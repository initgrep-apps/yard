# Blocking compression runs off the async runtime; panics unwind

`process_single_job` is an `async fn` with no `.await`, and every compression — process spawn, 30 ms `try_wait` poll, `fs::copy`, history append — runs on a tokio worker thread. `spawn_blocking` appears nowhere in the crate, so up to `maxConcurrent` runtime threads are parked per batch. Compounding this, the release profile sets `panic = "abort"`, which makes the `catch_unwind` in `queue.rs` and the `JoinError` handling in `scheduler.rs` unreachable in shipped builds: one processor panic kills the app. Both are corrected: the async layer only orchestrates, each job body runs in `spawn_blocking`, and release builds unwind so a single file's panic is reported as an error and the batch continues.

## Consequences

- The scheduler can use `JoinSet` instead of a hand-rolled `poll_fn` over `JoinHandle`s.
- A consent wait (ADR 0011) is possible only because the job body no longer blocks a runtime worker.
- Binary size grows slightly; the unwind cost is irrelevant next to process spawning.
- `get_history`, which is also `async` with no `.await` and reads the whole JSONL synchronously, moves to the same pattern.
