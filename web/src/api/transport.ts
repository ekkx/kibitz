/**
 * Every request in the app goes through `apiFetch`.
 *
 * With `VITE_MOCK=1` it is answered in-process by the mock server, which
 * returns real `Response` objects — SSE responses included, as byte streams
 * chopped at deliberately awkward offsets. That means mock mode exercises the
 * exact same client, SSE parser and rendering path as the real server; only
 * the origin of the bytes differs.
 *
 * The mock is reached through a **dynamic import inside the `IS_MOCK` branch**,
 * and never through a top-level one. `IS_MOCK` folds to a literal `false` at
 * build time, so a production bundle drops the branch and with it the only
 * reference to `mock/` — fixtures, fake engine and all. A static import would
 * ship every byte of the mock as dead code, which is how a real server once
 * ended up serving a fixture game.
 */
export const IS_MOCK = import.meta.env.VITE_MOCK === '1';

/** Resolved once and reused; the mock server keeps its sessions in module state. */
let mockFetchPromise: Promise<typeof fetch> | null = null;

function loadMockFetch(): Promise<typeof fetch> {
  mockFetchPromise ??= import('../mock/mockServer.ts').then((module) => module.mockFetch);
  return mockFetchPromise;
}

export const apiFetch: typeof fetch = IS_MOCK
  ? async (input, init) => (await loadMockFetch())(input, init)
  : (input, init) => fetch(input, init);

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }

  /** `409 {"error":"cancelled"}` — a newer analyze superseded this one. */
  get isCancelled(): boolean {
    return this.status === 409 && this.message === 'cancelled';
  }

  /** `409 {"error":"not analyzed"}` from `/explain`. */
  get isNotAnalyzed(): boolean {
    return this.status === 409 && this.message === 'not analyzed';
  }
}

/** Turn a non-2xx response into an `ApiError`, reading `{ "error": "..." }`. */
export async function raiseForStatus(response: Response): Promise<void> {
  if (response.ok) return;
  let message = `${response.status} ${response.statusText}`;
  try {
    const body = (await response.json()) as { error?: string };
    if (body && typeof body.error === 'string') message = body.error;
  } catch {
    /* non-JSON error body: keep the status line */
  }
  throw new ApiError(response.status, message);
}
