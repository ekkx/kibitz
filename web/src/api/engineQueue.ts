/**
 * One engine, one request at a time.
 *
 * The server drives a single Stockfish process, and `POST /analyze` cancels
 * whatever search is running (API.md). That rule is not limited to other
 * `analyze` calls: an `analyze` fired while `analyze-game` is streaming kills
 * the sweep, which then ends with
 * `event: error {"error":"engine: search was cancelled by a newer request"}` —
 * verified against the real server. Since the app now sweeps automatically on
 * open, and the user is free to click through moves while it runs, that race
 * would otherwise be the normal case rather than the exception.
 *
 * So every engine-bound request queues here and waits for the previous one to
 * finish. Requests whose signal aborted while they waited never start, which is
 * what makes clicking through five moves during a sweep cost one analysis
 * rather than five.
 */

let tail: Promise<unknown> = Promise.resolve();

export function runExclusive<T>(task: () => Promise<T>, signal?: AbortSignal): Promise<T> {
  const result = tail.then(() => {
    if (signal?.aborted) throw new DOMException('aborted before it reached the engine', 'AbortError');
    return task();
  });
  // The queue must keep moving even when a task fails or is aborted.
  tail = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}
