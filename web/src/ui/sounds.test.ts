import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { playSound, setSoundEnabled, soundForSan } from './sounds.ts';

describe('soundForSan', () => {
  it('picks the plain move sound for a quiet move', () => {
    expect(soundForSan('e4')).toBe('move');
    expect(soundForSan('Nf3')).toBe('move');
    expect(soundForSan('e8=Q')).toBe('move');
  });

  it('hears a capture', () => {
    expect(soundForSan('Nxe5')).toBe('capture');
    expect(soundForSan('exd5')).toBe('capture');
    // Disambiguated captures: the file letter before the `x` must not stop it
    // being recognised, which a stricter pattern than "contains x" would risk.
    expect(soundForSan('Nbxd2')).toBe('capture');
    expect(soundForSan('R1xa7')).toBe('capture');
  });

  it('hears castling on either side, in either notation', () => {
    expect(soundForSan('O-O')).toBe('castle');
    expect(soundForSan('O-O-O')).toBe('castle');
    expect(soundForSan('0-0')).toBe('castle');
    expect(soundForSan('0-0-0')).toBe('castle');
  });

  it('hears check', () => {
    expect(soundForSan('Qh5+')).toBe('check');
    expect(soundForSan('e8=Q+')).toBe('check');
  });

  it('hears checkmate', () => {
    expect(soundForSan('Qh7#')).toBe('checkmate');
    expect(soundForSan('e8=Q#')).toBe('checkmate');
  });

  /**
   * The precedence rule is the whole point of this function: one sound per move,
   * and the more important fact wins. A capture that gives check is *check*, not
   * a capture, and castling into mate is *mate*, not castling.
   */
  it('ranks mate over check over capture over castling', () => {
    expect(soundForSan('exd5+')).toBe('check');
    expect(soundForSan('Qxf7#')).toBe('checkmate');
    expect(soundForSan('Qxf7+')).toBe('check');
    expect(soundForSan('O-O+')).toBe('check');
    expect(soundForSan('O-O-O#')).toBe('checkmate');
  });

  it('ignores the annotations a PGN may have hung off the end', () => {
    expect(soundForSan('Nf3!?')).toBe('move');
    expect(soundForSan('Qxf7#!!')).toBe('checkmate');
    expect(soundForSan('Bxh7+?')).toBe('check');
  });
});

const EVENTS = ['move', 'capture', 'castle', 'check', 'checkmate'] as const;

const globals = globalThis as { AudioContext?: unknown; fetch?: unknown };
const realFetch = globalThis.fetch;

/**
 * Nothing in this file is allowed to touch the network, so the default origin
 * serves nothing at all. Scenarios below opt in to the files they want to exist,
 * which keeps each one's premise visible at its call site rather than inherited.
 */
beforeAll(() => {
  globals.fetch = fakeFetch([]);
});

afterAll(() => {
  globals.fetch = realFetch;
  delete globals.AudioContext;
});

/**
 * `playSound` runs here under vitest's node environment, where there is no
 * `AudioContext` at all — the same shape as a browser that has blocked audio, a
 * server render, or any future test that happens to import a component that
 * makes a sound. All of those have to be silent, not fatal.
 *
 * Order matters in this block: the module caches its context on first use, so
 * the no-audio case has to be exercised before the stub is installed. It is
 * checked with a `typeof` on every call rather than latched, which is what makes
 * the stub visible afterwards.
 */
describe('playSound without audio', () => {
  it('does not throw when there is no AudioContext', () => {
    for (const event of EVENTS) expect(() => playSound(event)).not.toThrow();
  });

  it('does not throw when toggled either way with no audio', () => {
    expect(() => setSoundEnabled(false)).not.toThrow();
    expect(() => playSound('move')).not.toThrow();
    // Also the first enable, and so the one that kicks off sample loading with
    // neither a context nor any files to find. That has to be silent too.
    expect(() => setSoundEnabled(true)).not.toThrow();
  });
});

// --- stubs -----------------------------------------------------------------

/**
 * A stub `AudioContext`, just complete enough to let the graph be built.
 *
 * This is not testing what anything sounds like — nothing short of a listener
 * can do that. It tests which of the two remaining outcomes each event reaches:
 * a sample, or silence. Every buffer source that starts is recorded along with
 * the file it came from, the detune, the trim offset and the gain it went
 * through, so "did this play my file, at the right level, from the right point?"
 * is an assertion rather than a guess.
 *
 * The node constructors that only the deleted synthesiser ever used throw rather
 * than returning stubs — see below.
 */
class StubParam {
  value = 1;
  setValueAtTime(value: number): void {
    this.value = value;
  }
  linearRampToValueAtTime(): void {}
  exponentialRampToValueAtTime(): void {}
}

class StubNode {
  readonly gain = new StubParam();
  readonly frequency = new StubParam();
  readonly Q = new StubParam();
  readonly playbackRate = new StubParam();
  type = '';
  buffer: unknown = null;
  /** Where this node was wired, so a source can be traced to its gain stage. */
  destination: StubNode | null = null;
  connect(destination: StubNode): StubNode {
    this.destination = destination;
    return destination;
  }
  disconnect(): void {}
}

interface Play {
  /** Which file this came from; see `sizeOf`. */
  from: number;
  /** The `playbackRate` it was given — the detune. */
  rate: number;
  /** The second argument to `start`, i.e. the trimmed leading silence. */
  offset: number;
  /** The gain it was played through — the per-event level. */
  level: number;
}

const SAMPLE_RATE = 44100;

/** A decoded buffer, standing in for whatever `decodeAudioData` would return. */
interface FakeBuffer {
  from: number;
  numberOfChannels: number;
  sampleRate: number;
  getChannelData: (index: number) => Float32Array;
}

class StubContext {
  readonly sampleRate = SAMPLE_RATE;
  readonly currentTime = 1.5;
  readonly destination = new StubNode();
  state: AudioContextState = 'running';
  resumed = 0;
  /** Sample one-shots started, with the variation each was given. */
  plays: Play[] = [];
  /** Which files this browser can decode, by `sizeOf` identity. */
  decodable: (from: number) => boolean = () => true;
  /** The PCM a decoded file is pretended to contain. */
  pcm: (from: number) => Float32Array[] = () => [silentThen(LEADING_SILENCE)];

  reset(): void {
    this.plays = [];
  }

  /** Nothing was played. With synthesis gone this is the only other outcome. */
  get silent(): boolean {
    return this.plays.length === 0;
  }

  createGain(): StubNode {
    return new StubNode();
  }
  /**
   * The synthesised sound engine was deleted, and these three are how that is
   * enforced rather than merely asserted. Every path through this module now
   * ends in either a sample or silence; anything that reaches for an oscillator,
   * a filter, or a hand-filled buffer is a resurrected fallback, and it fails
   * here loudly instead of leaking one wrong noise per page load into somebody's
   * ears. "Did not synthesise" is therefore not inferred from a count that
   * happened to be zero — it is unreachable by construction.
   */
  createOscillator(): never {
    throw new Error('synthesis was deleted: nothing may construct an oscillator');
  }
  createBiquadFilter(): never {
    throw new Error('synthesis was deleted: nothing may construct a filter');
  }
  createBuffer(): never {
    throw new Error('synthesis was deleted: nothing may build a buffer by hand');
  }
  createBufferSource(): StubNode & { start: (when: number, offset?: number) => void; stop: () => void } {
    const context = this;
    const node = new StubNode();
    return Object.assign(node, {
      start: (_when: number, offset = 0) => {
        const tagged = node.buffer as { from?: number } | null;
        context.plays.push({
          from: tagged?.from ?? -1,
          rate: node.playbackRate.value,
          offset,
          level: node.destination?.gain.value ?? -1,
        });
      },
      stop: () => {},
    });
  }
  decodeAudioData(bytes: ArrayBuffer): Promise<unknown> {
    // Byte length carries which file this was, so a play can be traced back to
    // the sample it came from — that is how the probe order and the castle rule
    // are checked.
    const from = bytes.byteLength;
    if (!this.decodable(from)) return Promise.reject(new Error('unsupported codec tag'));
    const channels = this.pcm(from);
    return Promise.resolve({
      from,
      numberOfChannels: channels.length,
      sampleRate: SAMPLE_RATE,
      getChannelData: (index: number) => channels[index] ?? new Float32Array(0),
    } satisfies FakeBuffer);
  }
  resume(): Promise<void> {
    this.resumed += 1;
    // Deferred on purpose: a real `resume` is asynchronous, so the state is
    // still `suspended` when `playSound` looks at it immediately afterwards.
    // That is precisely the case the "drop this one sound" rule exists for.
    return Promise.resolve().then(() => {
      this.state = 'running';
    });
  }
}

// --- synthetic PCM, for the onset trimming --------------------------------

/** Digital silence, then a hard transient — the shape of every real file here. */
const LEADING_SILENCE = 0.05;

function silentThen(seconds: number, total = 0.2, first = 0.9): Float32Array {
  const data = new Float32Array(Math.round(total * SAMPLE_RATE));
  const onset = Math.round(seconds * SAMPLE_RATE);
  data[onset] = first;
  // A short decaying tail after the transient, so the buffer is not one spike.
  for (let index = onset + 1; index < data.length; index += 1) {
    data[index] = first * 0.5 ** ((index - onset) / 400);
  }
  return data;
}

/**
 * What `onsetOffset` should return for a file whose sound starts at `seconds`:
 * the crossing, less the 3 ms backoff, in whole samples.
 */
function expectedOffset(seconds: number): number {
  const onset = Math.round(seconds * SAMPLE_RATE);
  return Math.max(0, onset - Math.ceil(0.003 * SAMPLE_RATE)) / SAMPLE_RATE;
}

/**
 * Hand the module this stub as its `AudioContext`.
 *
 * A `function` rather than an arrow, because the module calls `new Ctor()` and
 * arrows have no `[[Construct]]` — which fails as a `TypeError` inside the same
 * `try` that swallows a browser refusing to make a context, and would therefore
 * look exactly like "no audio available" rather than like a broken test.
 */
function installContext(stub: StubContext): void {
  globals.AudioContext = function AudioContextStub(): StubContext {
    return stub;
  };
}

/**
 * Every filename the loader can possibly ask for, in probe order per event.
 * The index is the file's identity, carried through the fake server and the
 * fake decoder as a byte length; see `decodeAudioData` above.
 */
const FILES = EVENTS.flatMap((event) => [`${event}.wav`, `${event}.mp3`]);
const sizeOf = (file: string): number => FILES.indexOf(file) + 1;

let fetches: string[] = [];

/**
 * The one deliberate log line, silenced and observed rather than left to print.
 *
 * Whether it fires is a design decision worth pinning down: it is the only
 * confirmation available to somebody who has just dropped files into
 * `web/public/sounds/` and wants to know whether the names were right, and it
 * must stay quiet for the overwhelming majority who have no files at all.
 */
const info = vi.spyOn(console, 'info').mockImplementation(() => {});

/** A fake origin serving exactly the files named, and 404 for everything else. */
function fakeFetch(present: readonly string[]) {
  return (input: unknown): Promise<unknown> => {
    const url = String(input);
    fetches.push(url);
    const file = FILES.find((candidate) => url === `/sounds/${candidate}`);
    if (!file || !present.includes(file)) {
      return Promise.resolve({ ok: false, status: 404, headers: { get: () => null } });
    }
    return Promise.resolve({
      ok: true,
      status: 200,
      headers: { get: () => 'audio/mpeg' },
      arrayBuffer: () => Promise.resolve(new ArrayBuffer(sizeOf(file))),
    });
  };
}

/** Every file for every event, the state of this developer's working tree. */
const ALL_MP3 = EVENTS.map((event) => `${event}.mp3`);

// --- no file, no sound -----------------------------------------------------

/**
 * The five samples ship, so this is the state of someone who has deleted them or
 * whose build is missing them — not the common case, but the one that decides
 * whether the module degrades or misbehaves.
 *
 * The stub throws on `createOscillator`, so "did not synthesise" is not being
 * inferred from a silent buffer count — a resurrected fallback would fail these
 * tests with an exception rather than quietly passing them.
 */
describe('playSound with audio and no samples', () => {
  const stub = new StubContext();

  beforeAll(() => {
    // Installed in `beforeAll` rather than in the describe body: describe bodies
    // all run during collection, before any test does, so assigning there would
    // hand the stub to the no-audio block above as well.
    installContext(stub);
  });

  afterAll(() => {
    delete globals.AudioContext;
    setSoundEnabled(true);
  });

  it.each(EVENTS)('is silent for %s, and does not reach for an oscillator', (event) => {
    stub.reset();
    stub.state = 'running';
    setSoundEnabled(true);
    expect(() => playSound(event)).not.toThrow();
    expect(stub.silent).toBe(true);
  });

  it('makes no sound at all when disabled', () => {
    stub.reset();
    setSoundEnabled(false);
    playSound('checkmate');
    expect(stub.silent).toBe(true);
  });
});

// --- the sample layer ------------------------------------------------------

/**
 * Each scenario gets a *fresh copy of the module*.
 *
 * The loader latches — it is meant to fetch once per session and never again —
 * so there is no way to re-run it within one module instance, and adding a reset
 * hook purely for tests would mean shipping a fifth export that production code
 * has no use for. `vi.resetModules()` plus a dynamic import is the same thing
 * without the API surface: every scenario is a new session.
 */
interface Options {
  /** Filenames the fake origin serves, e.g. `['move.mp3']`. */
  files?: readonly string[];
  /** Which of them this browser can decode, by `sizeOf` identity. */
  decodable?: (from: number) => boolean;
  /** What each decoded file is pretended to contain. */
  pcm?: (from: number) => Float32Array[];
}

async function session(options: Options = {}) {
  vi.resetModules();
  fetches = [];
  info.mockClear();

  const stub = new StubContext();
  if (options.decodable) stub.decodable = options.decodable;
  if (options.pcm) stub.pcm = options.pcm;
  installContext(stub);
  globals.fetch = fakeFetch(options.files ?? []);

  const sounds = await import('./sounds.ts');
  // One call does everything now: builds the context, fetches, decodes. A single
  // macrotask boundary drains the whole microtask queue behind it, so by the time
  // this returns the session is in the state a real one reaches within a few
  // milliseconds of the page loading — and, crucially, well before the first
  // move. That ordering is the entire reason the synthesised fallback existed
  // and the entire reason it no longer needs to.
  sounds.setSoundEnabled(true);
  await tick();

  stub.reset();
  return { sounds, stub };
}

const tick = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

/**
 * Undo the scenario's globals after each test.
 *
 * Registered per describe rather than once for the file: the synthesis block
 * above keeps one stub across all of its tests, and a file-wide teardown would
 * pull the context out from under it between them.
 */
const restoreGlobals = (): void => {
  delete globals.AudioContext;
  globals.fetch = fakeFetch([]);
};

// --- unlocking a context that was built before any user gesture ------------

/**
 * The context is now built on `setSoundEnabled`, which runs on mount — long
 * before anyone has clicked anything — so it starts `suspended` and the first
 * sound of every page load arrives while audio is still locked.
 *
 * That first sound must survive. `currentTime` is frozen while suspended, so
 * scheduling into it is not a loss: the sound fires the moment the context
 * starts running. What must not survive is a *backlog*, because if the unlock
 * never comes, everything queued behind it fires at once when it finally does.
 */
describe('a locked context', () => {
  afterEach(restoreGlobals);

  it('schedules the first sound and asks the context to resume', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3 });
    stub.state = 'suspended';
    stub.resumed = 0;
    stub.reset();

    sounds.playSound('move');

    expect(stub.resumed).toBe(1);
    // Not dropped: this is the click the user is waiting to hear.
    expect(stub.plays).toHaveLength(1);
  });

  it('drops everything queued behind it until the context is running', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3 });
    stub.state = 'suspended';
    stub.reset();

    for (let play = 0; play < 5; play += 1) sounds.playSound('move');

    // One waiting sound, not five arriving together on resume.
    expect(stub.plays).toHaveLength(1);
  });

  it('restores the allowance once the context is running', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3 });
    stub.state = 'suspended';
    stub.reset();
    sounds.playSound('move');
    sounds.playSound('move');
    expect(stub.plays).toHaveLength(1);

    stub.state = 'running';
    sounds.playSound('move');
    sounds.playSound('move');
    expect(stub.plays).toHaveLength(3);
  });

  it('plays nothing at all into a closed context', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3 });
    stub.state = 'closed';
    stub.resumed = 0;
    stub.reset();

    expect(() => sounds.playSound('move')).not.toThrow();
    expect(stub.silent).toBe(true);
    expect(stub.resumed).toBe(0);
  });
});

describe('samples', () => {
  afterEach(restoreGlobals);

  it('plays a sample for every event when every file is present', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3 });

    for (const event of EVENTS) {
      stub.reset();
      sounds.playSound(event);
      expect(stub.plays.map((play) => play.from)).toEqual([sizeOf(`${event}.mp3`)]);
      }
    // Said once, and only because something was actually found.
    expect(info).toHaveBeenCalledTimes(1);
  });

  /**
   * Per event, not all-or-nothing. Somebody who only has a checkmate sound they
   * like gets that one sound and the built-ins for the rest — which is the whole
   * reason resolution is a per-event lookup rather than a single "samples on"
   * flag.
   */
  it('resolves each event independently', async () => {
    const { sounds, stub } = await session({ files: ['checkmate.mp3'] });

    stub.reset();
    sounds.playSound('checkmate');
    expect(stub.plays).toHaveLength(1);
    for (const event of ['move', 'capture', 'check'] as const) {
      stub.reset();
      sounds.playSound(event);
      expect(stub.silent).toBe(true);
    }
  });

  /**
   * The castle rule. Castling being the one silent move in an otherwise audible
   * game reads as a bug, so a missing castle file is played as the move sample
   * twice instead.
   *
   * With all five files present this branch never runs — it is there for someone
   * who has deleted or replaced only some of them — which is exactly why it is
   * worth a test of its own rather than being left to be discovered.
   */
  it('builds a castle out of the move sample when there is no castle file', async () => {
    const { sounds, stub } = await session({ files: ['move.mp3'] });

    stub.reset();
    sounds.playSound('castle');
    // Twice, both times the move sample, and the second one attenuated.
    expect(stub.plays.map((play) => play.from)).toEqual([
      sizeOf('move.mp3'),
      sizeOf('move.mp3'),
    ]);
    // The level is castling's, not the move's, because gain follows the event
    // rather than the file it happened to be played from.
    expectLevel(stub.plays[0]!.level, 0.6);
    expectLevel(stub.plays[1]!.level, 0.6 * 0.8);
  });

  it('prefers a real castle sample when there is one', async () => {
    const { sounds, stub } = await session({ files: ['move.mp3', 'castle.mp3'] });

    stub.reset();
    sounds.playSound('castle');
    expect(stub.plays.map((play) => play.from)).toEqual([sizeOf('castle.mp3')]);
  });

  it('is silent for castling when there is no move sample either', async () => {
    const { sounds, stub } = await session({ files: ['checkmate.mp3'] });

    stub.reset();
    sounds.playSound('castle');
    expect(stub.silent).toBe(true);
  });

  it('detunes each play a little, and never enough to be heard as pitch', async () => {
    const { sounds, stub } = await session({ files: ['move.mp3'] });

    stub.reset();
    for (let play = 0; play < 40; play += 1) sounds.playSound('move');

    const rates = stub.plays.map((play) => play.rate);
    expect(rates).toHaveLength(40);
    for (const rate of rates) expect(Math.abs(rate - 1)).toBeLessThanOrEqual(0.02);
    // Varied rather than nominally varied: 40 identical rates would mean the
    // wobble is wired up but never actually applied.
    expect(new Set(rates).size).toBeGreaterThan(1);
  });
});

// --- which file, of the two extensions -------------------------------------

describe('extension probing', () => {
  afterEach(restoreGlobals);

  it('asks for the wav before the mp3, for every event', async () => {
    await session({ files: [] });

    expect(new Set(fetches)).toEqual(new Set(FILES.map((file) => `/sounds/${file}`)));
    // Per event, not across the whole batch: the events are probed concurrently,
    // so the wire order is every wav and then every mp3. What the rule is about
    // is which extension wins *for one event*, and that is this.
    for (const event of EVENTS) {
      expect(fetches.indexOf(`/sounds/${event}.wav`)).toBeLessThan(
        fetches.indexOf(`/sounds/${event}.mp3`),
      );
    }
  });

  it('takes the wav when both are there', async () => {
    const { sounds, stub } = await session({ files: ['move.wav', 'move.mp3'] });

    stub.reset();
    sounds.playSound('move');
    expect(stub.plays.map((play) => play.from)).toEqual([sizeOf('move.wav')]);
  });

  it('falls through to the mp3 when the wav will not decode', async () => {
    const { sounds, stub } = await session({
      files: ['move.wav', 'move.mp3'],
      decodable: (from) => from !== sizeOf('move.wav'),
    });

    stub.reset();
    sounds.playSound('move');
    expect(stub.plays.map((play) => play.from)).toEqual([sizeOf('move.mp3')]);
  });

  it('takes the mp3 when that is all there is', async () => {
    const { sounds, stub } = await session({ files: ['move.mp3'] });

    stub.reset();
    sounds.playSound('move');
    expect(stub.plays.map((play) => play.from)).toEqual([sizeOf('move.mp3')]);
  });
});

// --- trimming the leading silence -------------------------------------------

/**
 * These exercise `onsetOffset` through the offset it hands `start()`, which is
 * the only thing that offset is ever used for. The function itself is module
 * private on purpose — the module's contract is four names, and a fifth export
 * existing only for a test would be a worse trade than reading the value back
 * off the node the module built.
 */
describe('leading silence', () => {
  afterEach(restoreGlobals);

  const offsetFor = async (channels: Float32Array[]): Promise<number> => {
    const { sounds, stub } = await session({ files: ['move.mp3'], pcm: () => channels });
    stub.reset();
    sounds.playSound('move');
    expect(stub.plays).toHaveLength(1);
    return stub.plays[0]!.offset;
  };

  it('skips a known silent prefix, less the backoff', async () => {
    expect(await offsetFor([silentThen(0.05)])).toBeCloseTo(expectedOffset(0.05), 6);
  });

  it('starts at zero when the sound begins immediately', async () => {
    expect(await offsetFor([silentThen(0)])).toBe(0);
  });

  /** Must not scan off the end, and must not hand `start()` an offset at all. */
  it('starts at zero for a buffer that is silent throughout', async () => {
    expect(await offsetFor([new Float32Array(Math.round(0.2 * SAMPLE_RATE))])).toBe(0);
  });

  it('finds an onset whose first sample is negative', async () => {
    const negative = silentThen(0.05, 0.2, -0.9);
    expect(await offsetFor([negative])).toBeCloseTo(expectedOffset(0.05), 6);
  });

  /**
   * The earliest onset across channels wins. A stereo file whose right channel
   * leads the left by a millisecond must not be trimmed into its own attack.
   */
  it('takes the earliest onset across channels', async () => {
    const offset = await offsetFor([silentThen(0.05), silentThen(0.04)]);
    expect(offset).toBeCloseTo(expectedOffset(0.04), 6);
  });

  /** Below the −60 dBFS floor is silence however the peak scales. */
  it('does not mistake dither for the onset', async () => {
    const data = silentThen(0.05);
    for (let index = 0; index < Math.round(0.05 * SAMPLE_RATE); index += 1) {
      data[index] = index % 2 === 0 ? 0.0004 : -0.0004;
    }
    expect(await offsetFor([data])).toBeCloseTo(expectedOffset(0.05), 6);
  });
});

// --- headroom ---------------------------------------------------------------

/**
 * The guard against a sample that is hotter than the output can carry.
 *
 * Exercised through the gain the module actually schedules, for the same reason
 * the onset is: the decision function is module private, and the four-name
 * contract is worth more than a fifth export that only a test would call. Every
 * assertion here is on the realised gain, which is what a listener would hear.
 */
describe('headroom', () => {
  afterEach(restoreGlobals);

  /** Play `event` many times and report the realised gains. */
  async function levels(event: (typeof EVENTS)[number], peak: number, plays = 60) {
    const { sounds, stub } = await session({
      files: ALL_MP3,
      pcm: () => [silentThen(LEADING_SILENCE, 0.2, peak)],
    });
    stub.reset();
    for (let play = 0; play < plays; play += 1) sounds.playSound(event);
    return stub.plays.map((play) => play.level);
  }

  /**
   * The case that prompted this: a mastered file measuring +0.273 dBFS, played
   * at the one gain in the table that sits at unity. 1.0319 × 1.00 × 1.08 is
   * 1.114, and everything above 1.0 comes out as clipping.
   */
  it('caps a hot buffer so even the loudest realisation fits', async () => {
    const peak = 1.0319;
    for (const level of await levels('checkmate', peak)) {
      expect(level * peak).toBeLessThanOrEqual(1 + 1e-9);
    }
  });

  it('caps against the jittered gain, not the nominal one', async () => {
    const peak = 1.0319;
    const realised = await levels('checkmate', peak);
    // The cap is 1 / (peak × 1.08) = 0.897, so the *nominal* has come down from
    // 1.00 — capping the nominal at 1.00 and letting the wobble multiply it
    // afterwards would have produced values up to 1.08 here.
    expect(Math.max(...realised)).toBeLessThan(1 / peak + 1e-9);
    expect(Math.max(...realised)).toBeGreaterThan(0.9);
  });

  it('caps a buffer at exactly full scale', async () => {
    for (const level of await levels('checkmate', 1)) {
      expect(level).toBeLessThanOrEqual(1 + 1e-9);
    }
    // 1 / 1.08 = 0.926, so this is a real reduction rather than a no-op.
    const mean = (await levels('checkmate', 1)).reduce((a, b) => a + b, 0) / 60;
    expect(mean).toBeCloseTo(1 / 1.08, 1);
  });

  /**
   * A ceiling, not normalisation. This is the assertion that stops the guard
   * turning into a loudness maximiser the first time somebody "improves" it.
   */
  it('leaves a buffer inside the headroom completely alone', async () => {
    const realised = await levels('move', 0.5);
    for (const level of realised) {
      expect(level).toBeGreaterThanOrEqual(0.45 * 0.92 - 1e-9);
      expect(level).toBeLessThanOrEqual(0.45 * 1.08 + 1e-9);
    }
    // Never louder than asked for: 1 / (0.5 × 1.08) is 1.85, and if the guard
    // normalised rather than capped, that is what would have been applied.
    expect(Math.max(...realised)).toBeLessThan(0.5);
    // And the wobble survives the guard rather than being flattened by it.
    expect(new Set(realised).size).toBeGreaterThan(1);
  });

  /**
   * The other half of "not normalisation": with every event inside the headroom
   * the table's balance has to come through exactly as written, untouched.
   */
  it('does not change the balance between events that all fit', async () => {
    const mean = async (event: (typeof EVENTS)[number]): Promise<number> => {
      const realised = await levels(event, 0.5);
      return realised.reduce((total, level) => total + level, 0) / realised.length;
    };

    const move = await mean('move');
    const check = await mean('check');
    // 0.45 and 0.80 — the ratio the table declares, not one the guard invented.
    expect(check / move).toBeCloseTo(0.8 / 0.45, 1);
  });

  /** Peak 0: no division, no `Infinity`, no silence where a sound should be. */
  it('does nothing at all for a silent buffer', async () => {
    const { sounds, stub } = await session({
      files: ALL_MP3,
      pcm: () => [new Float32Array(Math.round(0.2 * SAMPLE_RATE))],
    });

    stub.reset();
    sounds.playSound('checkmate');
    expect(stub.plays).toHaveLength(1);
    const level = stub.plays[0]!.level;
    expect(Number.isFinite(level)).toBe(true);
    expectLevel(level, 1);
  });

  /**
   * Per event, so one hot file cannot drag the quiet events down with it. With a
   * full-scale buffer the ceiling lands at 0.926: `checkmate` (1.00) is above it
   * and comes down, `move` (0.45) is nowhere near it and does not move.
   */
  it('lowers only the events that need it', async () => {
    const { sounds, stub } = await session({
      files: ALL_MP3,
      pcm: () => [silentThen(LEADING_SILENCE, 0.2, 1)],
    });

    stub.reset();
    sounds.playSound('move');
    expectLevel(stub.plays[0]!.level, 0.45);

    stub.reset();
    sounds.playSound('checkmate');
    expectLevel(stub.plays[0]!.level, 1 / 1.08);
  });
});

// --- per-event level --------------------------------------------------------

/** Within the ±8 % play-to-play wobble of the nominal gain. */
function expectLevel(actual: number, nominal: number): void {
  expect(actual).toBeGreaterThanOrEqual(nominal * 0.92 - 1e-9);
  expect(actual).toBeLessThanOrEqual(nominal * 1.08 + 1e-9);
}

/**
 * The samples this was tuned against are all normalised to about −0.006 dBFS,
 * so without this table stepping through a game is four hundred full-scale hits
 * and the move / capture / check / mate hierarchy the synthesised set is built
 * around simply is not there.
 */
describe('per-event sample gain', () => {
  afterEach(restoreGlobals);

  const NOMINAL: Record<(typeof EVENTS)[number], number> = {
    move: 0.45,
    castle: 0.6,
    capture: 0.7,
    check: 0.8,
    checkmate: 1,
  };

  it('plays each event at its own level', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3 });

    for (const event of EVENTS) {
      stub.reset();
      sounds.playSound(event);
      expectLevel(stub.plays[0]!.level, NOMINAL[event]);
    }
  });

  it('keeps the move clearly under everything else', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3 });

    // Averaged over enough plays that the ±8 % wobble cannot invert the order.
    const mean = (event: (typeof EVENTS)[number]): number => {
      stub.reset();
      for (let play = 0; play < 60; play += 1) sounds.playSound(event);
      return stub.plays.reduce((total, play) => total + play.level, 0) / stub.plays.length;
    };

    // Rising in the order the table declares them, and the move well clear of
    // the bottom of that range rather than merely at it.
    const ordered = [mean('move'), mean('castle'), mean('capture'), mean('check'), mean('checkmate')];
    expect([...ordered].sort((a, b) => a - b)).toEqual(ordered);
    expect(ordered[0]!).toBeLessThan(ordered[2]! * 0.8);

    // Nothing runs away above unity — the jitter is a wobble on the level, not
    // a licence to exceed it by more than its own width.
    for (const level of ordered) expect(level).toBeLessThanOrEqual(1.08);
  });
});

describe('samples that are not there, or not usable', () => {
  afterEach(restoreGlobals);

  /** Every file gone: silent, and nothing said about it. */
  it('is silent for every event when no file is present', async () => {
    const { sounds, stub } = await session({ files: [] });

    for (const event of EVENTS) {
      stub.reset();
      expect(() => sounds.playSound(event)).not.toThrow();
      expect(stub.silent).toBe(true);
    }
    // Nothing found, so nothing said: a run of 404s must not put anything in
    // anyone's console.
    expect(info).not.toHaveBeenCalled();
  });

  /**
   * Some DAWs export AIFF in a proprietary compressed variant that
   * `decodeAudioData` cannot read, so a file being present and correctly named
   * is no guarantee that this browser can play it. It has to behave exactly like
   * a file that is absent — silent for that event, and silent about it.
   */
  it('is silent when the bytes arrive but cannot be decoded', async () => {
    const { sounds, stub } = await session({ files: ALL_MP3, decodable: () => false });

    for (const event of EVENTS) {
      stub.reset();
      expect(() => sounds.playSound(event)).not.toThrow();
      expect(stub.silent).toBe(true);
    }
    expect(info).not.toHaveBeenCalled();
  });

  it('does not go looking twice, however often sound is re-enabled', async () => {
    const { sounds } = await session({ files: ALL_MP3 });
    const first = fetches.length;
    // Two probes per event: the wav, then the mp3.
    expect(first).toBe(EVENTS.length * 2);

    sounds.setSoundEnabled(false);
    sounds.setSoundEnabled(true);
    sounds.setSoundEnabled(true);
    await tick();

    expect(fetches).toHaveLength(first);
  });

  it('does nothing at all where there is no fetch', async () => {
    vi.resetModules();
    const stub = new StubContext();
    installContext(stub);
    delete globals.fetch;

    const sounds = await import('./sounds.ts');
    expect(() => sounds.setSoundEnabled(true)).not.toThrow();
    await tick();

    stub.reset();
    sounds.playSound('move');
    expect(stub.silent).toBe(true);
  });
});
