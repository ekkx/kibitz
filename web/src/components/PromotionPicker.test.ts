import { describe, expect, it } from 'vitest';
import { squareOnScreen } from './PromotionPicker.tsx';

/**
 * The picker is four tiles at percentages of the board, so where a square is
 * *drawn* is the whole of its geometry — and it is the part that silently
 * breaks when the board is flipped or when Black is the one promoting.
 */
describe('squareOnScreen', () => {
  it('puts a1 bottom-left and h8 top-right for White', () => {
    expect(squareOnScreen('a1', 'white')).toEqual({ file: 0, row: 7 });
    expect(squareOnScreen('h8', 'white')).toEqual({ file: 7, row: 0 });
  });

  it('turns the board over for Black', () => {
    expect(squareOnScreen('a1', 'black')).toEqual({ file: 7, row: 0 });
    expect(squareOnScreen('h8', 'black')).toEqual({ file: 0, row: 7 });
  });

  it('places a promotion square on an edge row, whoever is promoting', () => {
    // Which is what lets the stack always run inwards: row 0 hangs down, row 7
    // hangs up, and there is no third case to get wrong.
    for (const orientation of ['white', 'black'] as const) {
      expect(squareOnScreen('b8', orientation)?.row).toBe(orientation === 'white' ? 0 : 7);
      expect(squareOnScreen('g1', orientation)?.row).toBe(orientation === 'white' ? 7 : 0);
    }
  });

  it('keeps the file of the promoting pawn', () => {
    expect(squareOnScreen('c8', 'white')?.file).toBe(2);
    expect(squareOnScreen('c8', 'black')?.file).toBe(5);
  });

  it('refuses anything that is not a square', () => {
    expect(squareOnScreen('z9' as never, 'white')).toBeNull();
    expect(squareOnScreen('a0' as never, 'white')).toBeNull();
  });
});
