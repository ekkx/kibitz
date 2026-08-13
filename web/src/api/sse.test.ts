import { describe, expect, it } from 'vitest';
import { SseParser, readSseStream, type SseEvent } from './sse.ts';

const STREAM =
  'event: progress\ndata: {"node_id":5,"done":5,"total":42}\n\n' +
  'event: node\ndata: {"fen":"8/8/8/8/8/8/8/8 w - - 0 1","depth":18}\n\n' +
  ': keep-alive comment\n\n' +
  'event: delta\ndata: {"text":"日本語のテキスト"}\n\n' +
  'event: done\ndata: {"total":42}\n\n';

function collect(chunks: string[]): SseEvent[] {
  const parser = new SseParser();
  const out: SseEvent[] = [];
  for (const c of chunks) out.push(...parser.push(c));
  out.push(...parser.flush());
  return out;
}

function expectStream(events: SseEvent[]) {
  expect(events.map((e) => e.event)).toEqual(['progress', 'node', 'delta', 'done']);
  expect(JSON.parse(events[0]!.data)).toEqual({ node_id: 5, done: 5, total: 42 });
  expect(JSON.parse(events[2]!.data)).toEqual({ text: '日本語のテキスト' });
}

describe('SseParser', () => {
  it('parses a whole stream delivered as one chunk', () => {
    expectStream(collect([STREAM]));
  });

  it('parses the same stream split at every single character boundary', () => {
    // The exhaustive version of "handle events split across chunk boundaries":
    // every possible two-way split must produce identical output.
    for (let i = 0; i <= STREAM.length; i++) {
      expectStream(collect([STREAM.slice(0, i), STREAM.slice(i)]));
    }
  });

  it('parses the stream one character at a time', () => {
    expectStream(collect([...STREAM]));
  });

  it('splits inside the blank-line separator', () => {
    const events = collect(['event: a\ndata: 1\n', '\nevent: b\ndata: 2\n\n']);
    expect(events).toEqual([
      { event: 'a', data: '1' },
      { event: 'b', data: '2' },
    ]);
  });

  it('handles CRLF line endings', () => {
    const events = collect(['event: a\r\ndata: {"x":1}\r\n\r\n']);
    expect(events).toEqual([{ event: 'a', data: '{"x":1}' }]);
  });

  it('joins multi-line data with newlines and keeps the default event name', () => {
    const events = collect(['data: line one\ndata: line two\n\n']);
    expect(events).toEqual([{ event: 'message', data: 'line one\nline two' }]);
  });

  it('emits a trailing event that never got its blank line', () => {
    const events = collect(['event: done\ndata: {"total":1}\n']);
    expect(events).toEqual([{ event: 'done', data: '{"total":1}' }]);
  });

  it('tolerates data lines with no space after the colon', () => {
    expect(collect(['event:delta\ndata:{"text":"hi"}\n\n'])).toEqual([
      { event: 'delta', data: '{"text":"hi"}' },
    ]);
  });
});

describe('readSseStream', () => {
  it('reassembles events across byte chunks that split UTF-8 characters', async () => {
    const bytes = new TextEncoder().encode(STREAM);
    // 3-byte chunks are guaranteed to cut some of the 3-byte CJK characters.
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (let i = 0; i < bytes.length; i += 3) controller.enqueue(bytes.slice(i, i + 3));
        controller.close();
      },
    });
    const events: SseEvent[] = [];
    await readSseStream(new Response(stream), (e) => events.push(e));
    expectStream(events);
  });
});
