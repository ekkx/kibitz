import { useCallback, useEffect, useState } from 'react';
import { getHealth } from '../api/client.ts';
import type { HealthResponse } from '../api/types.ts';

/**
 * `GET /api/health` on startup (API.md's expected flow, step 1).
 *
 * Three outcomes have to be told apart, because the remedy differs:
 * the server answered and the engine is up; the server answered but Stockfish
 * failed to start (`engine: null`); or the server is not there at all.
 */
export type HealthState =
  | { status: 'checking' }
  | { status: 'ok'; engine: string }
  | { status: 'no-engine' }
  | { status: 'unreachable' };

export function useHealth(): { health: HealthState; recheck: () => void } {
  const [health, setHealth] = useState<HealthState>({ status: 'checking' });
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setHealth({ status: 'checking' });
    getHealth()
      .then((response: HealthResponse) => {
        if (cancelled) return;
        setHealth(
          response.ok && response.engine
            ? { status: 'ok', engine: response.engine }
            : { status: 'no-engine' },
        );
      })
      .catch(() => {
        if (!cancelled) setHealth({ status: 'unreachable' });
      });
    return () => {
      cancelled = true;
    };
  }, [nonce]);

  const recheck = useCallback(() => setNonce((value) => value + 1), []);
  return { health, recheck };
}
