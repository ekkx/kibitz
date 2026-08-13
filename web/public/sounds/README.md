# Board sounds

The board's five sounds live here, and they ship with kibitz. Nothing has to be
set up: clone, build, and moves make a sound.

| File | Played when |
|---|---|
| `move.mp3` | A piece is moved |
| `capture.mp3` | A piece is taken |
| `castle.mp3` | Either side castles |
| `check.mp3` | The move gives check |
| `checkmate.mp3` | The move is mate |

Vite copies this directory into `dist` verbatim, so these are served from
`/sounds/` in development and in a built image alike.
[`src/ui/sounds.ts`](../../src/ui/sounds.ts) fetches and decodes them once, when
sound is first enabled, and plays them from memory after that.

## Replacing them

They are ordinary files. Overwrite one, or all five, and reload — there is no
build step, no manifest and no preprocessing.

Either extension works: `move.wav` is looked for before `move.mp3`, so a WAV you
drop in wins over the MP3 already here without your having to delete anything.
That order is not arbitrary — WAV is uncompressed PCM, decodes identically in
every browser, and carries none of the encoder padding that every MP3 begins
with. MP3 is accepted because it is what most exports and most sound libraries
give you.

Resolution is **per file**. Replace one and the other four are untouched. Delete
one and that event is silent — there is no synthesised stand-in, by design; see
the note at the top of [`src/ui/sounds.ts`](../../src/ui/sounds.ts).

The single exception is castling: if the castle file is missing but the move file
is present, castling plays the move sample twice rather than nothing at all.
Castling being the one silent move in an otherwise audible game reads as a bug,
and a piece landing twice is what castling is.

### Format

Anything the browser can decode with `decodeAudioData`. Uncompressed PCM WAV is
the safe choice; MP3 is fine. The extension is what the loader searches on, not
what it trusts — the bytes have to be decodable whatever the name says.

Watch out for files exported by a DAW in its own flavour of a familiar format.
Some export AIFF as a proprietary *compressed* variant, which no browser can
decode and which `ffmpeg` and macOS `afinfo` will also refuse; a `.aif` on disk
is not necessarily a `.aif` anything can read. Render to plain WAV or MP3 first,
and check anything you are unsure about:

```sh
afinfo move.wav          # macOS
ffprobe move.wav         # anywhere
```

A file that is present but undecodable behaves exactly like one that is absent:
that event is silent, and nothing is logged as an error.

### What you will see in devtools

One `404` per event, for the `.wav` that is looked for ahead of each shipped
`.mp3`. That is expected and is not worth fixing — with no manifest there is no
way to know what is there without asking. The console also gets a single `info`
line naming everything that loaded, which is how to confirm a replacement was
picked up and spelled correctly.

## What is done to the files

Three adjustments happen in the browser at load, so that a replacement works well
without your having to prepare it.

**Leading silence is trimmed.** Files routinely start with tens of milliseconds
of nothing — MP3 decoder padding, or simply where the render began relative to
the transient. The five here carry 48–64 ms of it. That is a long time for a
sound whose job is to coincide with a piece landing; past about 10 ms it stops
reading as feedback and starts reading as lag. kibitz finds the first sample
above the greater of −60 dBFS and −46 dB relative to the file's own peak, backs
off 3 ms so the attack is never clipped, and starts playback there. You do not
need to top-and-tail anything.

**Level is set per event.** Recordings tend to arrive normalised to full scale —
all five here measure within 0.01 dB of the ceiling — which flattens the
move / capture / check / mate hierarchy into four hundred identical full-scale
hits per game. Playback gain is therefore applied per event: the move sits about
7 dB under checkmate, with castle, capture and check in between. That is a mixing
judgement about *these* files. If your set already has its own dynamics, the
table at `SAMPLE_GAIN` in [`src/ui/sounds.ts`](../../src/ui/sounds.ts) is the
thing to flatten.

**Headroom is not required.** A file hotter than full scale is fine — one of the
five measures +0.27 dBFS, which is what anything that has been through a limiter
tends to look like. kibitz measures each buffer's peak at load and caps the gain
it applies so that even the loudest play cannot exceed the output ceiling. You do
not need to re-render a set to leave room at the top, and doing so will not make
it sound better here.

That is a ceiling and not normalisation: a file with plenty of headroom is played
at exactly the level the table asks for, never turned up. The cap is worked out
per event, so one hot file cannot pull the quieter events down with it.

Keep replacements short. A board sound that outlasts the click that caused it
feels laggy, and at the speed you can arrow through a game the tails pile up.
Each play also gets a small random detune (±2 %) and level wobble, so that four
hundred repetitions in one game do not become fatiguing — a recording is
byte-identical every time, and the ear notices.
