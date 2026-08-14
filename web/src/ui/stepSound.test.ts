import { describe, expect, it } from 'vitest';
import { stepSound } from './stepSound.ts';

/**
 * Positions from the Opera Game, which is the fixture the rest of the app is
 * exercised against and which happens to contain every case that matters: a
 * capture, a capture that gives check, a castle, and a mate.
 */
const AFTER_E4 = 'rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1';
const START = 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1';
/** After 11.Bxb5+ — Black is in check and has not answered it yet. */
const IN_CHECK = 'rn2kb1r/p3qppp/5n2/1B2p1B1/4P3/1Q6/PPP2PPP/R3K2R b KQkq - 0 11';
/** After 11…Nbd7, the same check now blocked. This is also the position 12.O-O-O is played from. */
const CHECK_ANSWERED = 'r3kb1r/p2nqppp/5n2/1B2p1B1/4P3/1Q6/PPP2PPP/R3K2R w KQkq - 1 12';
/** After 12.O-O-O. */
const AFTER_CASTLE = 'r3kb1r/p2nqppp/5n2/1B2p1B1/4P3/1Q6/PPP2PPP/2KR3R b kq - 2 12';

const forward = (san: string, landedFen = START) => stepSound({ san, forward: true, landedFen });
const back = (landedFen: string, san = 'ignored') =>
  stepSound({ san, forward: false, landedFen });

describe('stepSound going forward', () => {
  it('reads the move from its notation', () => {
    expect(forward('e4')).toBe('move');
    expect(forward('Nxb5')).toBe('capture');
    expect(forward('O-O-O')).toBe('castle');
    expect(forward('Bxb5+')).toBe('check');
    expect(forward('Rd8#')).toBe('checkmate');
  });

  it('ignores the position it lands in — the notation is already the answer', () => {
    // Belt and braces: a forward step must not start consulting the FEN, or
    // `Bxb5+` would come out a capture and `Rd8#` a check.
    expect(forward('Rd8#', IN_CHECK)).toBe('checkmate');
    expect(forward('e4', IN_CHECK)).toBe('move');
  });
});

describe('stepSound going back', () => {
  it('never replays the sound of the move it is undoing', () => {
    // The bug this rule exists for: stepping back off `Rd8#` used to announce
    // checkmate at the moment the position stopped being one.
    expect(back(CHECK_ANSWERED)).toBe('move');
    expect(back(AFTER_E4)).toBe('move');
    expect(back(START)).toBe('move');
  });

  it('is a check when the position stepped back onto is one', () => {
    // Taking back the move that answered a check puts the check back on the
    // board. That is true in the present tense, so it is heard.
    expect(back(IN_CHECK)).toBe('check');
  });

  it('is a castle when the step being undone is one', () => {
    // The exception to "backwards, the notation is not consulted": undoing a
    // castle still moves king and rook, which is the whole of what the castle
    // sound claims. Nothing about it inverts, unlike a capture or a mate.
    expect(back(CHECK_ANSWERED, 'O-O-O')).toBe('castle');
    expect(back(CHECK_ANSWERED, '0-0-0')).toBe('castle');
    // And the check half of `O-O+` still inverts even though the castling half
    // does not, so this is a castle rather than the check the notation names.
    expect(back(CHECK_ANSWERED, 'O-O+')).toBe('castle');
  });

  it('is a move for the step that undoes the castle from the other side', () => {
    // Stepping *forward* onto the castled position is the castle; the move
    // after it is an ordinary one, so returning to the castled position is too.
    expect(forward('O-O-O', AFTER_CASTLE)).toBe('castle');
    expect(back(AFTER_CASTLE, 'Rd8')).toBe('move');
  });

  it('is a move on an unreadable FEN rather than nothing at all', () => {
    // A position the client cannot parse is not a reason for the board to go
    // silent: a piece still moved.
    expect(back('not a fen')).toBe('move');
  });
});
