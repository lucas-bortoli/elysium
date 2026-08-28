import { Font, drawText, measureText } from "ely:framebuffer";
import type { Color } from "ely:framebuffer";

/** Greedy word-wrap: splits `text` on single spaces and packs as many words
 * as fit within `maxWidth` per line, measured in `font`. A word wider than
 * `maxWidth` on its own still gets its own line and overflows — there is no
 * hyphenation or mid-word breaking. Newlines in `text` are not treated
 * specially; split on them yourself first if you need hard breaks. */
export function wrapTextSpaces(
  text: string,
  maxWidth: number,
  font: Font = Font.Cozette,
): string[] {
  const lines: string[] = [];
  let line = "";
  for (const word of text.split(" ")) {
    const candidate = line === "" ? word : `${line} ${word}`;
    if (measureText(candidate, font).width > maxWidth && line !== "") {
      lines.push(line);
      line = word;
    } else {
      line = candidate;
    }
  }
  if (line !== "") lines.push(line);
  return lines;
}

/** Draws `text` so its left edge sits at `x` — the same as `drawText`,
 * included so all three alignments read alike at a call site. */
export function drawTextLeftAligned(
  x: number,
  y: number,
  text: string,
  color: Color,
  font: Font = Font.Cozette,
): void {
  drawText(x, y, text, color, font);
}

/** Draws `text` centred horizontally on `centerX`. */
export function drawTextCentered(
  centerX: number,
  y: number,
  text: string,
  color: Color,
  font: Font = Font.Cozette,
): void {
  drawText(centerX - measureText(text, font).width / 2, y, text, color, font);
}

/** Draws `text` so its right edge ends at `right`. */
export function drawTextRightAligned(
  right: number,
  y: number,
  text: string,
  color: Color,
  font: Font = Font.Cozette,
): void {
  drawText(right - measureText(text, font).width, y, text, color, font);
}

/** Draws each of `lines` stacked from `y` downward, advancing by the font's
 * line height (optionally scaled by `lineSpacing`). Returns the y just past
 * the last line, so callers can keep laying out below it. */
export function drawTextBlock(
  x: number,
  y: number,
  lines: string[],
  color: Color,
  font: Font = Font.Cozette,
  lineSpacing = 1,
): number {
  const step = measureText("X", font).height * lineSpacing;
  let cursor = y;
  for (const line of lines) {
    drawText(x, cursor, line, color, font);
    cursor += step;
  }
  return cursor;
}
