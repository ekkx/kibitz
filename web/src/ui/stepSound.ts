import { checkedColor } from '../chess/rules.ts';
import { soundForSan, type SoundEvent } from './sounds.ts';

/**
 * Which sound one step of navigation makes.
 *
 * Forwards, the SAN says everything, because that is what SAN was written for:
 * `Rd8#` is a checkmate, `Bxb5+` is a check, `Nxb5` is a capture.
 *
 * Backwards it mostly says the opposite of the truth, because SAN has a tense —
 * and the rule that sorts out which parts of it survive being run backwards is
 * one question asked of each fact the notation carries: **does this event still
 * mean the same thing with time reversed?**
 *
 * Almost nothing does. A capture inverts: undoing `Nxb5` puts a piece back on
 * the board rather than taking one off, and the capture sound is a claim about
 * a piece being removed. Mate inverts hardest of all: stepping back off `Rd8#`
 * is the position *ceasing* to be checkmate, and announcing that with the mate
 * sound tells the ear the game just ended at the exact moment it is being
 * un-ended. Check inverts too, but symmetrically, so it is worth re-deriving:
 * the check that mattered a moment ago is gone, but the position stepped back
 * onto may itself be a check — the one that had just been answered — and that
 * is a fact about the board in the present tense.
 *
 * **Castling is the exception, and the only one.** It is not a claim about
 * something being gained, lost or threatened; it is a claim about *how many
 * pieces moved*. Two did, king and rook, and two still do when the step is
 * taken backwards — the same two pieces travelling the same two paths, in the
 * other direction. There is nothing in that to invert. So castling keeps its
 * own sound going back, and stepping over `O-O` in either direction sounds like
 * what it is.
 *
 * That leaves the backward order: castle, then check, then move. Nothing rides
 * on castle coming first — a position with the castling side to move cannot be
 * a check, or the castle would have been illegal — but the order is written to
 * follow the principle rather than to exploit that coincidence.
 *
 * Checkmate is unreachable backwards, and not by accident: mate ends the game,
 * so no node has a mated position as its parent and no backward step can land
 * on one. The mate sound therefore only ever plays forwards, on the move that
 * delivers it, which is the only moment it is true.
 */
export function stepSound(move: { san: string; forward: boolean; landedFen: string }): SoundEvent {
  if (move.forward) return soundForSan(move.san);
  if (CASTLING.test(move.san)) return 'castle';
  return checkedColor(move.landedFen) ? 'check' : 'move';
}

/**
 * Whether this SAN is a castle at all — which is a different question from the
 * one `soundForSan` answers, and the reason the test is written out here rather
 * than borrowed from it.
 *
 * `soundForSan` resolves a *precedence*: exactly one sound per move, with `#`
 * and `+` ranked above castling, so `O-O+` comes back as a check. That is the
 * right answer going forwards. Going backwards the check is the half that
 * inverts and the castling is the half that does not, so the backward rule has
 * to ask about castling on its own, unranked.
 *
 * Zeroes are accepted alongside letter O for the same reason `soundForSan`
 * accepts them: PGN in the wild writes `0-0`.
 */
const CASTLING = /^[O0]-[O0]/;
