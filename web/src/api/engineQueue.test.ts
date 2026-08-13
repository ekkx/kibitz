import { describe, expect, it } from 'vitest';
import { runExclusive } from './engineQueue.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('runExclusive', () => {
  it('runs one task at a time, in order', async () => {
    const first = deferred<string>();
    const started: string[] = [];

    const a = runExclusive(() => {
      started.push('a');
      return first.promise;
    });
    const b = runExclusive(async () => {
      started.push('b');
      return 'b';
    });

    // `b` must not have touched the engine while `a` holds it.
    await Promise.resolve();
    expect(started).toEqual(['a']);

    first.resolve('a');
    await expect(a).resolves.toBe('a');
    await expect(b).resolves.toBe('b');
    expect(started).toEqual(['a', 'b']);
  });

  it('never starts a task whose signal aborted while it waited', async () => {
    const blocker = deferred<void>();
    const held = runExclusive(() => blocker.promise);

    const controller = new AbortController();
    let started = false;
    const queued = runExclusive(async () => {
      started = true;
    }, controller.signal);

    controller.abort();
    blocker.resolve();
    await held;
    await expect(queued).rejects.toThrow();
    expect(started).toBe(false);
  });

  it('keeps the queue moving after a task fails', async () => {
    const failing = runExclusive(() => Promise.reject(new Error('engine died')));
    await expect(failing).rejects.toThrow('engine died');
    await expect(runExclusive(async () => 'next')).resolves.toBe('next');
  });
});
