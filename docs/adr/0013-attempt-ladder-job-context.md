# One attempt ladder and one job context

About 25% of processor code is the same orchestration written four times: the "already under target" shortcut, the per-attempt `Ok / Err / Cancelled` match, best-attempt tracking, the best-effort tail, attempt cleanup, and a binary quality search that exists twice. The `CompressionProcessor` method takes eight arguments and every processor re-derives its own context. Replace the arguments with a `JobContext` (input, output, target, floors, cancel token, progress, resolved sidecars, temp dir, policy) and replace the duplicated state with a `Ladder` object in `engine/` that owns the shortcut, attempt recording, best tracking, cancellation cleanup, the best-effort tail, and attempt-file removal. The processor keeps its own ladder order — that part is engine knowledge — but every attempt goes through `ladder.record(...)`, which returns `Continue` or `Stop(result)`.

A fully declarative attempt list was rejected: image, PDF, video and document attempts are not the same kind of thing, and forcing them into one `AttemptSpec` trait would replace duplicated control flow with duplicated boilerplate. The state machine removes the duplication without pretending the engines are uniform.

## Considered Options

- **Job context + declarative ladder (chosen).** One place for cancellation, cleanup and recording; processors shrink to engine-specific knowledge.
- **Keep four ladders, extract helpers.** Cheaper now, but consent and progress would still be implemented four times.
- **Trait with associated types or async methods.** Buys nothing here; the processors are not generic over anything.

## Consequences

- Consent, attempt cleanup and history recording exist once.
- The progress percentage is computed by the ladder, not by each engine wrapper.
- Each processor's attempt order stays readable in its own file.
- `ProcessorRegistry` is constructed once per app, not once per job.
