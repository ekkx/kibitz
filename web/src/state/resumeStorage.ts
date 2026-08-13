/**
 * Which session the tab was looking at, so a reload does not throw the game
 * away.
 *
 * Only the identifiers are kept. The tree, and every analysis on it, live on
 * the server and come back from `GET /api/sessions/{id}` — copying them into
 * localStorage would mean two sources of truth for the same data, and the
 * stale one would win on the next reload.
 */
const KEY = 'kibitz.session';

export interface ResumePoint {
  sessionId: string;
  nodeId: number;
}

export function readResumePoint(): ResumePoint | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<ResumePoint>;
    if (typeof parsed?.sessionId !== 'string' || typeof parsed.nodeId !== 'number') return null;
    return { sessionId: parsed.sessionId, nodeId: parsed.nodeId };
  } catch {
    return null;
  }
}

export function writeResumePoint(point: ResumePoint): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(point));
  } catch {
    /* private mode: this tab simply will not resume */
  }
}

export function clearResumePoint(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* nothing to clear */
  }
}
