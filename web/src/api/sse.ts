/**
 * A minimal but correct `text/event-stream` reader.
 *
 * `EventSource` only does GET, and two of our three streaming endpoints
 * (`/analyze-game`, `/ask`) are POSTs, so we read the body ourselves.
 *
 * The thing that has to be right: network chunks have nothing to do with event
 * boundaries. A single `data:` line can arrive as three chunks, the blank line
 * that terminates an event can land in the next chunk, and a multi-byte UTF-8
 * character can be cut in half. `SseParser` keeps a buffer across pushes and
 * `TextDecoder({stream:true})` keeps the byte halves together.
 */

export interface SseEvent {
  /** The `event:` field, or `"message"` when absent, per the spec. */
  event: string;
  /** All `data:` lines joined with "\n". */
  data: string;
  id?: string;
}

export class SseParser {
  private buffer = '';

  /** Feed decoded text; returns every complete event it now contains. */
  push(text: string): SseEvent[] {
    this.buffer += text;
    const events: SseEvent[] = [];

    // An event ends at a blank line. Normalise CRLF so one rule covers both.
    // Note we only consume up to the last complete separator — the remainder
    // stays buffered for the next chunk.
    for (;;) {
      const sep = findSeparator(this.buffer);
      if (!sep) break;
      const raw = this.buffer.slice(0, sep.index);
      this.buffer = this.buffer.slice(sep.index + sep.length);
      const parsed = parseEventBlock(raw);
      if (parsed) events.push(parsed);
    }
    return events;
  }

  /** Called at end of stream: emit a trailing event that had no blank line. */
  flush(): SseEvent[] {
    const rest = this.buffer;
    this.buffer = '';
    const parsed = rest.trim().length > 0 ? parseEventBlock(rest) : null;
    return parsed ? [parsed] : [];
  }
}

function findSeparator(buf: string): { index: number; length: number } | null {
  let best: { index: number; length: number } | null = null;
  for (const [sep, length] of [
    ['\r\n\r\n', 4],
    ['\n\n', 2],
    ['\r\r', 2],
  ] as const) {
    const i = buf.indexOf(sep);
    if (i !== -1 && (best === null || i < best.index)) best = { index: i, length };
  }
  return best;
}

function parseEventBlock(block: string): SseEvent | null {
  let event = 'message';
  let id: string | undefined;
  const dataLines: string[] = [];

  for (const line of block.split(/\r\n|\n|\r/)) {
    if (line === '' || line.startsWith(':')) continue; // comment / keep-alive
    const colon = line.indexOf(':');
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? '' : line.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);

    if (field === 'event') event = value;
    else if (field === 'data') dataLines.push(value);
    else if (field === 'id') id = value;
    // `retry` and unknown fields are ignored.
  }

  if (dataLines.length === 0 && event === 'message') return null;
  return id === undefined
    ? { event, data: dataLines.join('\n') }
    : { event, data: dataLines.join('\n'), id };
}

/**
 * Drive a `Response` body through `SseParser`, invoking `onEvent` per event.
 * Resolves when the stream ends; rejects if `signal` aborts mid-stream.
 */
export async function readSseStream(
  response: Response,
  onEvent: (event: SseEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const body = response.body;
  if (!body) throw new Error('response has no body');

  const reader = body.getReader();
  const decoder = new TextDecoder();
  const parser = new SseParser();

  const abort = () => void reader.cancel().catch(() => {});
  signal?.addEventListener('abort', abort);

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      for (const ev of parser.push(decoder.decode(value, { stream: true }))) onEvent(ev);
    }
    const tail = decoder.decode();
    for (const ev of parser.push(tail)) onEvent(ev);
    for (const ev of parser.flush()) onEvent(ev);
  } finally {
    signal?.removeEventListener('abort', abort);
  }
}

/** `JSON.parse` that returns null instead of throwing on a malformed frame. */
export function parseEventData<T>(data: string): T | null {
  try {
    return JSON.parse(data) as T;
  } catch {
    return null;
  }
}
