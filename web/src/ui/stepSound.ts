import { checkedColor } from '../chess/rules.ts';
import { soundForSan, type SoundEvent } from './sounds.ts';

/**
 * Which sound one step of navigation makes.
 *
 * Forwards, the SAN says everything, because that is what SAN was written for:
 * `Rd8#` is a checkmate, `Bxb5+` is a check, `Nxb5` is a capture.
 *
 * Backwards it says the opposite of the truth, because SAN has a tense. Stepping
 * back off `Rd8#` is the position *ceasing* to be checkmate, and announcing that
 * with the mate sound tells the ear the game just ended at the exact moment it
 * is being un-ended. The rest of the notation inverts the same way: undoing
 * `Nxb5` puts a piece back rather than taking one, and undoing `O-O` is not
 * castling.
 *
 * So a backward step is described by **the position it lands in** rather than by
 * the move it undoes. A piece slides, which is a move; and if the position
 * arrived at is itself a check — stepping back onto the check that had just been
 * answered — that is a fact about the board in the present tense, so it is heard.
 *
 * Checkmate is unreachable this way, and not by accident: mate ends the game, so
 * no node has a mated position as its parent and no backward step can land on
 * one. The mate sound therefore only ever plays forwards, on the move that
 * delivers it, which is the only moment it is true.
 */
export function stepSound(move: { san: string; forward: boolean; landedFen: string }): SoundEvent {
  if (move.forward) return soundForSan(move.san);
  return checkedColor(move.landedFen) ? 'check' : 'move';
}
