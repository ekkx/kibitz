/**
 * Board sounds, played from sample files.
 *
 * Five ship with kibitz, in `web/public/sounds/`: `move`, `capture`, `castle`,
 * `check` and `checkmate`. Vite copies that directory into `dist` verbatim, so
 * they are served from `/sounds/` in dev and in a built image alike, and a fresh
 * clone has working board sounds with no setup.
 *
 * They are assets on disk rather than anything baked into the bundle, which is
 * the reason the loader below is written the way it is: any of them can be
 * replaced with a different recording, or deleted outright, and everything still
 * has to behave. `web/public/sounds/README.md` documents that from the outside.
 *
 * ## No sample, no sound
 *
 * An event with no usable file is silent. There is no synthesised stand-in, and
 * that is deliberate rather than unfinished — this module used to carry a
 * complete synthesised set, oscillators and filtered noise bursts and a
 * per-event timbre table, several hundred lines of it, and it was deleted for
 * two reasons worth recording so that nobody rebuilds it.
 *
 * **An approximation that is audibly not the thing it stands in for is worse
 * than nothing.** The synthesised click was a decent wooden click. It was not
 * *the* click, and next to a real sample it did not read as a substitute — it
 * read as a fault. A user hearing it among four hundred correct sounds does not
 * think "ah, the fallback"; they think something is broken, and they are not
 * wrong to. Silence, by contrast, is unambiguous: nothing happened, so nothing
 * was heard, and there is nothing to misdiagnose.
 *
 * **A fallback that is only ever reached by accident is not a fallback.** It was
 * reached exactly once per page load, by the one event that beat the decoder to
 * the finish line, and never deliberately by anybody. That is not a safety net;
 * it is a second implementation nobody is listening to, with its own bugs, its
 * own tests and its own maintenance cost, whose entire observable contribution
 * was one wrong noise per session.
 *
 * ## Resolution
 *
 * Per event, not all-or-nothing: each event independently gets whichever file
 * answers for it, and an event whose file is missing or undecodable is simply
 * silent. Replacing one recording and leaving the rest alone works exactly as
 * you would expect, and a 404 is a normal outcome rather than an error — see
 * `loadSamples`, which is quiet about all of it.
 *
 * The one exception, and the only fallback in the module, is castling: a missing
 * castle file is played as the *move* sample twice (`render`). That is a
 * fallback between real samples rather than a synthesised stand-in, and it is
 * what someone who deleted one of the five should hear.
 *
 * Either extension is accepted, `.wav` before `.mp3` — see `SAMPLE_EXTENSIONS`
 * for why that order.
 *
 * ## What is done to the files
 *
 * Three things, rather than being demanded of whoever supplies them, because
 * requiring an `ffmpeg` step of anyone who wants to swap a sound would defeat
 * the point:
 *
 *   - **Leading silence is trimmed at load** (`analyse`). Real files start with
 *     tens of milliseconds of nothing — encoder padding, or just where the
 *     render began — and fifty milliseconds of latency on a click that is
 *     supposed to coincide with a piece landing reads as lag.
 *   - **Level is set per event** (`SAMPLE_GAIN`), because recordings tend to
 *     arrive normalised to full scale and therefore flat, with none of the
 *     dynamic hierarchy a board wants.
 *   - **Gain is capped to fit the output** (`headroomGain`), because a file can
 *     be hotter than full scale and clipping is not a sound anyone chose. The
 *     alternative is a rule about export headroom that nobody would remember.
 *
 * Each play also gets a small random `playbackRate` detune and a slight level
 * wobble (`SAMPLE_DETUNE`, `SAMPLE_LEVEL_WOBBLE`), because a recording is
 * byte-identical every time and four hundred identical clicks in one game is
 * fatiguing in a way that a few cents of variation quietly fixes.
 *
 * ## Lifecycle
 *
 * Everything now happens at one moment: the first `setSoundEnabled(true)`, which
 * `useSettings` calls once on mount. The `AudioContext` is built, and the files
 * are fetched and decoded into it.
 *
 * This reverses an earlier decision, and the reversal is the point. The context
 * used to be built lazily on the first `playSound`, to avoid leaving a
 * `suspended` context holding an audio device open before the user had done
 * anything. But decoding needs a context, so the samples could not be decoded
 * until that first play — and the first sound of every page load came out of the
 * synthesiser instead, which is the bug that got the synthesiser deleted. With
 * nothing to fall back to, the trade is no longer close: a suspended context is
 * legal, costs essentially nothing, and is the only thing standing between a
 * fetched file and being audible. It is resumed on the first play (`unlock`),
 * which is the first moment a user gesture makes that possible.
 *
 * An `OfflineAudioContext` would also decode without touching an audio device,
 * and was considered. It is not used: it decodes at whatever sample rate it was
 * constructed with rather than the output device's, so every buffer would be
 * resampled twice — once on decode and again on playback — and it relies on
 * `AudioBuffer`s being portable between contexts, which is one more thing to be
 * true. One context, one resample, no portability assumption.
 *
 * Nothing here ever throws. It is called from render-adjacent code where an
 * exception would take the board down over a sound effect, and the module is
 * imported by tests that run under node, where neither `AudioContext` nor
 * `fetch` need exist. Every such case resolves to "make no sound", silently.
 */

export type SoundEvent = 'move' | 'capture' | 'castle' | 'check' | 'checkmate';

/**
 * Which sound a SAN string earns.
 *
 * Exactly one sound per move, so this is a precedence order rather than a set of
 * independent flags: `#` beats `+` beats `x` beats castling. A capture that
 * gives check is deliberately the *check* sound — the two facts are not equally
 * interesting, and the user needs to hear the one that changes what they have to
 * do next. `O-O` scores below `x` only because the two cannot co-occur.
 *
 * Suffix annotations (`!`, `?!`, `!?`) ride along harmlessly: they never contain
 * any of the characters tested for. Zeroes are accepted alongside letter O
 * because plenty of PGN in the wild writes castling as `0-0`, and a mis-sounded
 * move is a silly thing to lose to a typographic convention.
 *
 * `ui/stepSound.ts` sits on top of this and decides what a *backward* step
 * sounds like, which SAN cannot answer because SAN has a tense.
 */
export function soundForSan(san: string): SoundEvent {
  if (san.includes('#')) return 'checkmate';
  if (san.includes('+')) return 'check';
  if (san.includes('x')) return 'capture';
  if (CASTLING.test(san)) return 'castle';
  return 'move';
}

const CASTLING = /^[O0]-[O0]/;

/**
 * Play it.
 *
 * Silently does nothing when sound is off, when there is no `AudioContext` to be
 * had, when no sample is loaded for this event, or when the browser has not
 * unlocked audio yet and one sound is already waiting on that.
 */
export function playSound(event: SoundEvent): void {
  if (!enabled) return;
  const audio = context();
  if (!audio) return;

  // Belt and braces: `useSettings` calls `setSoundEnabled` on mount, so loading
  // has normally been under way for a while by now. This covers a caller that
  // never touched the setting at all, and is latched, so it costs one boolean.
  ensureLoading(audio);

  if (!unlock(audio)) return;

  try {
    // A small lookahead rather than `currentTime` itself: scheduling at exactly
    // the current time means the first sample or two have already gone past by
    // the time the graph is built.
    render(audio, event, audio.ctx.currentTime + LOOKAHEAD);
  } catch {
    /* a browser that dislikes something in the graph should not break the board */
  }
}

/**
 * Global on/off, driven by the `sound` setting (`state/useSettings.ts`), and the
 * cue to build the context and go and get the files.
 *
 * A module-level flag rather than a parameter on `playSound`, because the call
 * sites are move handlers that have no business knowing about preferences, and
 * threading a boolean through every one of them is exactly the kind of thing
 * that gets forgotten in the one code path nobody tested.
 *
 * Turning sound off deliberately leaves the `AudioContext` open. It is a handful
 * of kilobytes, closing it is irreversible, and the user who just muted the
 * board is the likeliest person in the world to unmute it a minute later.
 */
export function setSoundEnabled(on: boolean): void {
  enabled = on;
  if (!on) return;

  // Build the context here rather than on the first play, so that decoding can
  // finish long before anyone moves a piece. See the Lifecycle note above.
  const audio = context();
  if (audio) ensureLoading(audio);
}

/**
 * What one event actually plays.
 *
 * A loaded sample wins. Castling has a second chance — see below — and anything
 * still unresolved is silence, which is the whole contract of this module rather
 * than a failure inside it.
 *
 * **Castle is special.** If someone supplied `move` but no `castle`, playing
 * nothing at all would make castling the one move in a game that is silent,
 * which reads as a bug. So a missing castle sample is built out of the move
 * sample instead: the same two impacts at the same `CASTLE_GAP`, with the same
 * `CASTLE_SECOND_LEVEL` attenuation on the rook. The user hears their own sound
 * played twice, which is what castling is anyway.
 */
function render(audio: Audio, event: SoundEvent, at: number): void {
  // Level follows the *event*, never the file it happens to be played from —
  // which is what makes the castle-from-move case below sound like castling
  // rather than like two moves.
  const level = SAMPLE_GAIN[event];

  const sample = samples.get(event);
  if (sample) {
    playSample(audio, at, sample, level);
    return;
  }

  if (event === 'castle') {
    const move = samples.get('move');
    if (move) {
      playSample(audio, at, move, level);
      playSample(audio, at + CASTLE_GAP, move, level * CASTLE_SECOND_LEVEL);
    }
  }
}

/**
 * Ask a locked context to unlock, and say whether this sound should be scheduled.
 *
 * Browsers start an `AudioContext` created outside a user gesture in the
 * `suspended` state, and this one is created on mount, so the first sound of
 * every page load arrives while it is still locked. `resume()` is asynchronous,
 * so the context is *still* suspended when this returns — but `currentTime` is
 * frozen while suspended, which means a sound scheduled now plays the moment the
 * context starts running rather than being lost. That is exactly what should
 * happen to the first click of a session.
 *
 * What must not happen is a *backlog*. If audio never unlocks — no gesture, a
 * policy that refuses — every queued sound would fire simultaneously the instant
 * it does. So exactly one sound may wait on the unlock: the one that makes the
 * unlock audible. Anything after it is dropped until the context is running,
 * which resets the allowance.
 */
function unlock(audio: Audio): boolean {
  const state = audio.ctx.state;
  if (state === 'running') {
    queuedWhileLocked = 0;
    return true;
  }
  // 'closed', or anything a future spec invents: nothing can be scheduled.
  if (state !== 'suspended') return false;

  // Fire and forget: the caller is not waiting, and a rejection here just means
  // audio is still locked.
  void audio.ctx.resume().catch(() => {});

  if (queuedWhileLocked > 0) return false;
  queuedWhileLocked += 1;
  return true;
}

// --- levels and timing ------------------------------------------------------

/** Master level. Set to be clearly audible on laptop speakers without ever
 *  being the loudest thing on the user's desktop — a board is not a game. */
const MASTER = 0.3;

/** Scheduling margin, in seconds. See `playSound`. */
const LOOKAHEAD = 0.005;

/** How much quieter the rook is than the king, when castling is built out of
 *  the move sample. */
const CASTLE_SECOND_LEVEL = 0.8;

/** The gap between the two castling impacts. Around 50 ms is the width of a
 *  drummer's flam — two events, unmistakably one intention. */
const CASTLE_GAP = 0.05;

interface Audio {
  ctx: AudioContext;
  /** Everything routes through here, so level lives in exactly one place. */
  out: GainNode;
}

// --- samples ---------------------------------------------------------------

/** Where a sample for each event is looked for. `web/public/` is copied into
 *  `dist` verbatim by Vite, so this path is the same in dev and in a built
 *  image. */
const SAMPLE_PATH = '/sounds/';

/**
 * Extensions probed, in order, for each event.
 *
 * WAV first because it is the better artefact when both exist: uncompressed PCM
 * decodes identically in every browser and carries no encoder delay, whereas
 * every MP3 begins with a block of decoder padding that has to be trimmed back
 * off (see `analyse`). MP3 second because it is what most people actually
 * have — it is what a DAW exports by default and what a sound library hands you
 * — and telling someone to re-render a file the browser can already decode
 * would be a pointless errand.
 *
 * These two are *probed*, not required: anything `decodeAudioData` accepts works
 * if it is named with one of these extensions.
 */
const SAMPLE_EXTENSIONS = ['wav', 'mp3'] as const;

/** Every event, in one iterable, for the loader. */
const SOUND_EVENTS = ['move', 'capture', 'castle', 'check', 'checkmate'] as const satisfies
  readonly SoundEvent[];

/**
 * Playback gain per event.
 *
 * Recordings tend to arrive normalised to the ceiling — the five shipped files
 * measure within about 0.01 dB of full scale, which is what a mastering chain
 * produces. Played back flat, stepping through a game with the arrow key is four
 * hundred full-scale hits and there is no hierarchy at all between an ordinary
 * move and mate.
 *
 * So the hierarchy is imposed here, spanning about 7 dB:
 *
 * | event     | gain | vs. mate | why |
 * |-----------|------|----------|-----|
 * | move      | 0.45 | −6.9 dB  | heard hundreds of times; must sit under everything |
 * | castle    | 0.60 | −4.4 dB  | twice a game, and it is two impacts already |
 * | capture   | 0.70 | −3.1 dB  | something was taken — it should land |
 * | check     | 0.80 | −1.9 dB  | the one mid-game event that demands a response |
 * | checkmate | 1.00 | —        | the game is over; nothing follows it |
 *
 * **This is a mixing judgement about the five shipped files, not a law.** It is
 * the first thing to change for anyone who replaces them — a set that is not
 * normalised to full scale may already carry its own dynamics, in which case
 * flattening this table towards unity is the right move.
 */
const SAMPLE_GAIN: Record<SoundEvent, number> = {
  move: 0.45,
  castle: 0.6,
  capture: 0.7,
  check: 0.8,
  checkmate: 1,
};

/**
 * The anti-fatigue variation.
 *
 * A recording is the same 200 milliseconds every single time, and the ear picks
 * that up as mechanical within a few dozen plays. `playbackRate` is the cheap
 * fix: ±2 % is about a third of a semitone, comfortably below the threshold at
 * which anyone hears it *as* pitch but well above the threshold at which the
 * repetition stops being noticeable. The level wobble does the same for
 * dynamics; a real hand does not put a piece down with identical force twice.
 *
 * Both are deliberately smaller than they could be. The failure mode of getting
 * this wrong is not "insufficiently varied", it is "the board is broken", so it
 * errs towards the side where the user never consciously notices it at all.
 */
const SAMPLE_DETUNE = 0.02;
const SAMPLE_LEVEL_WOBBLE = 0.08;

/** The most the level wobble can ever multiply a gain by. The headroom guard
 *  has to reason about the loudest realisation, not the nominal one. */
const MAX_LEVEL_WOBBLE = 1 + SAMPLE_LEVEL_WOBBLE;

/**
 * A decoded sample, plus the two facts about its contents that playback needs.
 * Both come out of one walk of the channel data at decode time (`analyse`).
 */
interface Sample {
  buffer: AudioBuffer;
  /** Seconds of leading silence to skip, ready for `start(when, offset)`. */
  offset: number;
  /** Peak absolute amplitude, for the headroom guard in `headroomGain`. */
  peak: number;
}

/** Decoded samples, by event. A missing entry means that event is silent. */
const samples = new Map<SoundEvent, Sample>();

/** One `AudioBufferSourceNode`, trimmed, detuned and level-wobbled a hair. */
function playSample(audio: Audio, at: number, sample: Sample, level: number): void {
  const source = audio.ctx.createBufferSource();
  source.buffer = sample.buffer;
  source.playbackRate.setValueAtTime(1 + wobbleBy(SAMPLE_DETUNE), at);

  const gain = audio.ctx.createGain();
  // No envelope: a one-shot carries its own, and imposing one on top would fight
  // whatever shape the user recorded. Just a level — the nominal one for this
  // event, pulled down first if this particular file needs the room.
  const nominal = headroomGain(level, sample.peak);
  gain.gain.setValueAtTime(nominal * (1 + wobbleBy(SAMPLE_LEVEL_WOBBLE)), at);
  gain.connect(audio.out);

  source.connect(gain);
  // The second argument is where in the buffer to begin — this is the whole
  // point of having measured the onset.
  source.start(at, sample.offset);
}

/** A symmetric random multiplier offset in ±`spread`. */
function wobbleBy(spread: number): number {
  return (Math.random() * 2 - 1) * spread;
}

/**
 * The nominal gain for an event, lowered if this file cannot carry it.
 *
 * **What this protects against.** A sample can be hotter than full scale. One of
 * the reference files measures +0.273 dBFS — peak 1.0319 — which is entirely
 * normal for anything that has been through a limiter, and which nothing warns
 * you about because the file itself is perfectly valid. Multiply that by
 * `SAMPLE_GAIN.checkmate` at unity and then by up to +8 % of level wobble and
 * the graph is asking for 1.08, which the output stage cannot deliver. What
 * comes out is clipped: a hard, buzzy edge on the one sound in the whole app
 * that is supposed to feel final. Nobody chose that, and nobody would connect it
 * to a mastering decision made in a completely different tool.
 *
 * **Why here.** The alternative is a rule in the README — "leave 3 dB of headroom
 * when you export" — which is exactly the kind of instruction that is read once
 * and never remembered, and which cannot be checked. The code already walks
 * every sample of every buffer to find the onset, so it knows the peak for free,
 * and enforcing the ceiling costs one multiplication per play. A rule the
 * machine can keep should not be delegated to a person.
 *
 * This is also what makes `SAMPLE_GAIN.checkmate = 1.00` safe to leave at unity:
 * the table can express "this event is as loud as it gets" without every entry
 * having to be discounted defensively against the worst file anyone might supply.
 *
 * **It is a ceiling, not normalisation.** A buffer already inside the headroom is
 * returned untouched, at exactly the level the table asked for — this must never
 * make a quiet file *louder*, and it must never change the balance between events
 * that all fit. Because the cap is computed per event against that event's own
 * nominal gain, a hot file also cannot drag the quiet events down with it: with a
 * full-scale buffer the ceiling lands at 0.926, so `checkmate` (1.00) comes down
 * and `move` (0.45) does not move at all.
 *
 * The peak is measured on the *decoded* buffer, which is what actually plays.
 * Chrome clamps on decode, so a +0.273 dBFS file arrives reading exactly 1.0000
 * and the guard still does the right thing — it is protecting the output stage,
 * not auditing the file.
 */
function headroomGain(nominal: number, peak: number): number {
  // A silent buffer has nothing to clip, and dividing by its peak would be a
  // very loud way to find out it was silent.
  if (!(peak > 0)) return nominal;
  // Against the *jittered* gain, not the nominal one: capping the nominal value
  // would still let a +8 % realisation go over the top.
  return Math.min(nominal, 1 / (peak * MAX_LEVEL_WOBBLE));
}

/**
 * Both facts playback needs about a decoded buffer: its peak, and where the
 * sound in it actually starts.
 *
 * Measured together because they are two questions about the same samples, and
 * because the onset threshold is itself derived from the peak — so they cannot
 * be answered independently anyway. One full walk collects both the peak and the
 * first crossing of the *absolute* floor. Only if the peak-relative threshold
 * turns out to be the binding one is there a second look, and that one starts at
 * the crossing already found and stops at the first sample above the higher
 * threshold, so it covers a few milliseconds rather than the whole file.
 *
 * ## The onset
 *
 * Files arrive with leading silence. The five this was tuned against begin with
 * 48–64 ms of it (measured at −60 dBFS): part is MP3 decoder padding, which
 * every MP3 has, and part is simply where the render was started relative to the
 * transient. Fifty milliseconds is a long time for a sound whose entire job is
 * to be simultaneous with a piece landing — well past the point where a UI click
 * stops feeling like feedback and starts feeling like lag.
 *
 * This is fixed here, at load, rather than by asking anyone to run `ffmpeg`
 * first. Doing it in the browser means it works for whatever file gets dropped
 * in later, and there is no preprocessed artefact to drift out of step with the
 * original it came from.
 *
 * **Threshold.** `max(ONSET_FLOOR, peak × ONSET_RELATIVE)` — −60 dBFS, the
 * conventional digital-silence floor, or −46 dB below the file's own peak,
 * whichever is higher. Relative to the peak rather than absolute so that a
 * quietly-rendered file is not measured against a threshold that is 5 % of its
 * entire dynamic range; floored absolutely so that a *silent* file cannot drive
 * the threshold to zero and match its own dither.
 *
 * The risk being traded here is asymmetric, and this errs the safe way. Too low
 * and some silence survives — the click is a millisecond late, which nobody
 * notices. Too high and it eats the attack transient, which for a percussive
 * sound is the part that carries its identity: trim the crack off a wooden click
 * and what is left is the body tone, which is a completely different sound. On
 * the reference files, raising the threshold from −60 dBFS to −26 dBFS moves the
 * crossing by 5 ms on `move` and 31 ms on the longest one, so there is real
 * attack in that region and this stays well below it.
 *
 * **Backoff.** Three milliseconds, subtracted from the crossing, for two
 * reasons. It guarantees the attack is never clipped even when the rise is slow
 * — the mate sample swells rather than cracks, and its crossing sits 5.9 ms
 * after its −60 dBFS point. And it puts the start back inside the near-silence,
 * so playback begins from something close to zero: starting mid-waveform is a
 * step discontinuity, i.e. an audible pop. On the reference files this lands at
 * −54 to −99 dBFS, comfortably inaudible, and leaves at most 3 ms of the
 * original latency behind.
 *
 * Pure, and takes raw channel data rather than an `AudioBuffer`, so it can be
 * exercised against arrays with known contents. A buffer that is silent
 * throughout reports a peak of 0 and an offset of 0, rather than running off the
 * end or handing `start()` an offset past the end of the buffer, which some
 * implementations throw on.
 */
function analyse(channels: readonly Float32Array[], sampleRate: number): Omit<Sample, 'buffer'> {
  let peak = 0;
  let floorAt = Number.POSITIVE_INFINITY;

  for (const data of channels) {
    let crossed = false;
    for (let index = 0; index < data.length; index += 1) {
      const level = Math.abs(data[index]!);
      if (level > peak) peak = level;
      if (!crossed && level >= ONSET_FLOOR) {
        crossed = true;
        if (index < floorAt) floorAt = index;
      }
    }
  }

  if (peak <= 0 || sampleRate <= 0 || !Number.isFinite(floorAt)) return { peak, offset: 0 };

  // The absolute floor is where the walk above already stopped looking. When the
  // peak-relative threshold is higher, the real onset is somewhere at or after
  // that point — never before it — so the rescan starts there rather than at
  // zero, and every channel gets to start there because the earliest crossing
  // across all of them bounds every individual one.
  const threshold = Math.max(ONSET_FLOOR, peak * ONSET_RELATIVE);
  let onset = floorAt;
  if (threshold > ONSET_FLOOR) {
    onset = Number.POSITIVE_INFINITY;
    for (const data of channels) {
      // `index < onset` because a later channel only matters if it starts earlier
      // than the best found so far — the earliest onset across channels wins, so
      // that a stereo file with one quiet side is not trimmed into its own attack.
      for (let index = floorAt; index < data.length && index < onset; index += 1) {
        if (Math.abs(data[index]!) >= threshold) {
          onset = index;
          break;
        }
      }
    }
  }
  if (!Number.isFinite(onset)) return { peak, offset: 0 };

  return {
    peak,
    offset: Math.max(0, onset - Math.ceil(ONSET_BACKOFF * sampleRate)) / sampleRate,
  };
}

/** Absolute silence floor, −60 dBFS. */
const ONSET_FLOOR = 0.001;
/** Relative silence floor, −46 dB below the file's own peak. */
const ONSET_RELATIVE = 0.005;
/** How far back from the crossing to start, in seconds. */
const ONSET_BACKOFF = 0.003;

/** Pair a decoded buffer with what `analyse` found in it. */
function measure(buffer: AudioBuffer): Sample {
  const channels: Float32Array[] = [];
  for (let index = 0; index < buffer.numberOfChannels; index += 1) {
    channels.push(buffer.getChannelData(index));
  }
  return { buffer, ...analyse(channels, buffer.sampleRate) };
}

// --- loading ---------------------------------------------------------------

/**
 * Look for the sample files, and decode each one as soon as it lands. Never
 * throws, never rejects.
 *
 * **Every failure here is routine.** The shipped files are `.mp3`, so the `.wav`
 * probe ahead of each one answers 404 every time on a stock checkout — five of
 * them, on every page load, entirely as intended. Add a file that is missing on
 * someone else's machine, or delete one, and the count changes; none of it is a
 * degraded state worth reporting. Network error, 404, HTML from a dev-server
 * fallback, bytes the browser cannot decode: all resolve to "that event is
 * silent", quietly and per event.
 *
 * The extensions are tried in order and the first that *decodes* wins, so a WAV
 * that turns out to be some format this browser has never heard of falls through
 * to the MP3 beside it instead of taking the event down with it. Probing stops
 * at the first success, so a complete set of WAVs costs five requests rather
 * than ten.
 *
 * The one thing worth saying out loud is the *positive* case, which is why the
 * single `console.info` names what was found. Somebody who has just swapped a
 * file in needs a way to confirm it was picked up and spelled correctly, and the
 * devtools console is the only channel this module has.
 */
async function loadSamples(audio: Audio): Promise<void> {
  // Node, or a browser old enough to lack `fetch`: nothing to load, and a
  // relative URL has no meaning without a document to resolve it against.
  if (typeof fetch !== 'function') return;

  await Promise.all(
    SOUND_EVENTS.map(async (event) => {
      for (const extension of SAMPLE_EXTENSIONS) {
        const bytes = await probe(`${SAMPLE_PATH}${event}.${extension}`);
        if (!bytes) continue;
        const sample = await decode(audio, bytes);
        if (sample) {
          samples.set(event, sample);
          return;
        }
      }
    }),
  );

  if (samples.size > 0) {
    console.info(
      `kibitz: using board sound samples from ${SAMPLE_PATH} for ${[...samples.keys()].join(', ')}`,
    );
  }
}

/** One candidate file. Resolves to its bytes, or to null for every kind of "no". */
async function probe(url: string): Promise<ArrayBuffer | null> {
  try {
    const response = await fetch(url);
    if (!response.ok) return null;
    // A dev server with an SPA fallback can answer a missing asset with
    // `index.html` and a cheerful 200. Feeding that to `decodeAudioData` fails
    // eventually anyway, but as a confusing decode error rather than as the
    // plain "no such file" it actually is.
    if (response.headers.get('content-type')?.startsWith('text/') === true) return null;
    return await response.arrayBuffer();
  } catch {
    /* offline, blocked, or no such file */
    return null;
  }
}

/** Decode one candidate, or null if this browser cannot. */
async function decode(audio: Audio, bytes: ArrayBuffer): Promise<Sample | null> {
  try {
    const buffer = await audio.ctx.decodeAudioData(bytes);
    // Guarded rather than trusted: the callback-only form of `decodeAudioData`
    // resolves to `undefined`, and a `Map` holding one of those would fail much
    // further away from the cause.
    return buffer ? measure(buffer) : null;
  } catch {
    /* not something this browser can decode: the caller tries the next one */
    return null;
  }
}

// --- context ---------------------------------------------------------------

let enabled = true;
let audio: Audio | null = null;
/** Latched by the first enable, so toggling the setting does not re-fetch. */
let loading = false;
/** How many sounds are waiting on an unlock. See `unlock`. */
let queuedWhileLocked = 0;
/** Latched when construction *threw*. A missing constructor is re-checked every
 *  time because that test is free; a constructor that blows up is not. */
let broken = false;

/** Start loading once, and only once. */
function ensureLoading(ready: Audio): void {
  if (loading) return;
  loading = true;
  void loadSamples(ready);
}

/**
 * The context and its master gain, built on demand.
 *
 * `webkitAudioContext` is still the only constructor on older iOS Safari, and it
 * is one line to support. `globalThis` is read through a cast rather than the
 * DOM lib's globals because the DOM lib declares `AudioContext` as always
 * present, which is exactly the assumption that has to be checked here — under
 * vitest's node environment neither name exists.
 */
function context(): Audio | null {
  if (audio) return audio;
  if (broken) return null;

  const globals = globalThis as {
    AudioContext?: typeof AudioContext;
    webkitAudioContext?: typeof AudioContext;
  };
  const Ctor = globals.AudioContext ?? globals.webkitAudioContext;
  if (!Ctor) return null;

  try {
    const ctx = new Ctor();
    const out = ctx.createGain();
    out.gain.setValueAtTime(MASTER, ctx.currentTime);
    out.connect(ctx.destination);
    audio = { ctx, out };
    return audio;
  } catch {
    broken = true;
    return null;
  }
}
