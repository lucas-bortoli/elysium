// The Framebuffer device: a fixed-resolution GPU surface programs draw to.
// Colors are always one of `Color`'s named entries (the kernel's fixed
// palette), never raw RGBA channels a program could get wrong.

import type { Size2d, Vector2d } from "ely:math";
import type { DrawTickerId } from "ely:framebuffer";
import type { Image, ImageId } from "ely:image";

declare function __framebuffer_clear_screen(color: Color): void;
declare function __framebuffer_fill_rectangle(
  x: number,
  y: number,
  w: number,
  h: number,
  color: Color,
): void;
declare function __framebuffer_draw_image(
  id: number,
  x: number,
  y: number,
): void;
declare function __framebuffer_draw_text(
  x: number,
  y: number,
  text: string,
  font: number,
  scale: number,
  color: Color,
): void;
declare function __framebuffer_measure_text(
  text: string,
  font: number,
): [number, number];
declare function __framebuffer_nearest_color(
  r: number,
  g: number,
  b: number,
): Color;
declare function __framebuffer_set_scale(scale: number): void;
declare function __framebuffer_path_begin(): void;
declare function __framebuffer_path_move_to(x: number, y: number): void;
declare function __framebuffer_path_line_to(x: number, y: number): void;
declare function __framebuffer_path_quad_to(
  cx: number,
  cy: number,
  x: number,
  y: number,
): void;
declare function __framebuffer_path_cubic_to(
  c1x: number,
  c1y: number,
  c2x: number,
  c2y: number,
  x: number,
  y: number,
): void;
declare function __framebuffer_path_close(): void;
declare function __framebuffer_path_rect(
  x: number,
  y: number,
  w: number,
  h: number,
): void;
declare function __framebuffer_path_oval(
  cx: number,
  cy: number,
  rx: number,
  ry: number,
): void;
declare function __framebuffer_path_rounded_rect(
  x: number,
  y: number,
  w: number,
  h: number,
  radius: number,
): void;
declare function __framebuffer_path_arc(
  cx: number,
  cy: number,
  r: number,
  start: number,
  end: number,
): void;
declare function __framebuffer_fill_path(color: Color, rule: FillRule): void;
declare function __framebuffer_stroke_path(
  color: Color,
  thickness: number,
  cap: LineCap,
  join: LineJoin,
): void;
declare function __framebuffer_push_transform(
  sx: number,
  ky: number,
  kx: number,
  sy: number,
  tx: number,
  ty: number,
): void;
declare function __framebuffer_pop_transform(): void;
declare function __framebuffer_push_clip(rule: FillRule): void;
declare function __framebuffer_pop_clip(): void;
declare function __framebuffer_draw_image_transformed(
  id: number,
  source: [number, number, number, number],
  transform: [number, number, number, number, number, number],
): void;
declare function __image_width(id: number): number;
declare function __image_height(id: number): number;
declare function __framebuffer_set_pixel(
  x: number,
  y: number,
  color: Color,
): void;

// The kernel's fixed, curated color palette. Every color a program can
// draw with is one of these named entries — never a raw, unconstrained
// RGBA value.
//
// <generated from kernel/framebuffer/palette.rs>
export const Color = {
  Red50: 0,
  Red100: 1,
  Red200: 2,
  Red300: 3,
  Red400: 4,
  Red500: 5,
  Red600: 6,
  Red700: 7,
  Red800: 8,
  Red900: 9,
  Red950: 10,
  Orange50: 11,
  Orange100: 12,
  Orange200: 13,
  Orange300: 14,
  Orange400: 15,
  Orange500: 16,
  Orange600: 17,
  Orange700: 18,
  Orange800: 19,
  Orange900: 20,
  Orange950: 21,
  Amber50: 22,
  Amber100: 23,
  Amber200: 24,
  Amber300: 25,
  Amber400: 26,
  Amber500: 27,
  Amber600: 28,
  Amber700: 29,
  Amber800: 30,
  Amber900: 31,
  Amber950: 32,
  Yellow50: 33,
  Yellow100: 34,
  Yellow200: 35,
  Yellow300: 36,
  Yellow400: 37,
  Yellow500: 38,
  Yellow600: 39,
  Yellow700: 40,
  Yellow800: 41,
  Yellow900: 42,
  Yellow950: 43,
  Lime50: 44,
  Lime100: 45,
  Lime200: 46,
  Lime300: 47,
  Lime400: 48,
  Lime500: 49,
  Lime600: 50,
  Lime700: 51,
  Lime800: 52,
  Lime900: 53,
  Lime950: 54,
  Green50: 55,
  Green100: 56,
  Green200: 57,
  Green300: 58,
  Green400: 59,
  Green500: 60,
  Green600: 61,
  Green700: 62,
  Green800: 63,
  Green900: 64,
  Green950: 65,
  Emerald50: 66,
  Emerald100: 67,
  Emerald200: 68,
  Emerald300: 69,
  Emerald400: 70,
  Emerald500: 71,
  Emerald600: 72,
  Emerald700: 73,
  Emerald800: 74,
  Emerald900: 75,
  Emerald950: 76,
  Teal50: 77,
  Teal100: 78,
  Teal200: 79,
  Teal300: 80,
  Teal400: 81,
  Teal500: 82,
  Teal600: 83,
  Teal700: 84,
  Teal800: 85,
  Teal900: 86,
  Teal950: 87,
  Cyan50: 88,
  Cyan100: 89,
  Cyan200: 90,
  Cyan300: 91,
  Cyan400: 92,
  Cyan500: 93,
  Cyan600: 94,
  Cyan700: 95,
  Cyan800: 96,
  Cyan900: 97,
  Cyan950: 98,
  Sky50: 99,
  Sky100: 100,
  Sky200: 101,
  Sky300: 102,
  Sky400: 103,
  Sky500: 104,
  Sky600: 105,
  Sky700: 106,
  Sky800: 107,
  Sky900: 108,
  Sky950: 109,
  Blue50: 110,
  Blue100: 111,
  Blue200: 112,
  Blue300: 113,
  Blue400: 114,
  Blue500: 115,
  Blue600: 116,
  Blue700: 117,
  Blue800: 118,
  Blue900: 119,
  Blue950: 120,
  Indigo50: 121,
  Indigo100: 122,
  Indigo200: 123,
  Indigo300: 124,
  Indigo400: 125,
  Indigo500: 126,
  Indigo600: 127,
  Indigo700: 128,
  Indigo800: 129,
  Indigo900: 130,
  Indigo950: 131,
  Violet50: 132,
  Violet100: 133,
  Violet200: 134,
  Violet300: 135,
  Violet400: 136,
  Violet500: 137,
  Violet600: 138,
  Violet700: 139,
  Violet800: 140,
  Violet900: 141,
  Violet950: 142,
  Purple50: 143,
  Purple100: 144,
  Purple200: 145,
  Purple300: 146,
  Purple400: 147,
  Purple500: 148,
  Purple600: 149,
  Purple700: 150,
  Purple800: 151,
  Purple900: 152,
  Purple950: 153,
  Fuchsia50: 154,
  Fuchsia100: 155,
  Fuchsia200: 156,
  Fuchsia300: 157,
  Fuchsia400: 158,
  Fuchsia500: 159,
  Fuchsia600: 160,
  Fuchsia700: 161,
  Fuchsia800: 162,
  Fuchsia900: 163,
  Fuchsia950: 164,
  Pink50: 165,
  Pink100: 166,
  Pink200: 167,
  Pink300: 168,
  Pink400: 169,
  Pink500: 170,
  Pink600: 171,
  Pink700: 172,
  Pink800: 173,
  Pink900: 174,
  Pink950: 175,
  Rose50: 176,
  Rose100: 177,
  Rose200: 178,
  Rose300: 179,
  Rose400: 180,
  Rose500: 181,
  Rose600: 182,
  Rose700: 183,
  Rose800: 184,
  Rose900: 185,
  Rose950: 186,
  Slate50: 187,
  Slate100: 188,
  Slate200: 189,
  Slate300: 190,
  Slate400: 191,
  Slate500: 192,
  Slate600: 193,
  Slate700: 194,
  Slate800: 195,
  Slate900: 196,
  Slate950: 197,
  Gray50: 198,
  Gray100: 199,
  Gray200: 200,
  Gray300: 201,
  Gray400: 202,
  Gray500: 203,
  Gray600: 204,
  Gray700: 205,
  Gray800: 206,
  Gray900: 207,
  Gray950: 208,
  Zinc50: 209,
  Zinc100: 210,
  Zinc200: 211,
  Zinc300: 212,
  Zinc400: 213,
  Zinc500: 214,
  Zinc600: 215,
  Zinc700: 216,
  Zinc800: 217,
  Zinc900: 218,
  Zinc950: 219,
  Neutral50: 220,
  Neutral100: 221,
  Neutral200: 222,
  Neutral300: 223,
  Neutral400: 224,
  Neutral500: 225,
  Neutral600: 226,
  Neutral700: 227,
  Neutral800: 228,
  Neutral900: 229,
  Neutral950: 230,
  Stone50: 231,
  Stone100: 232,
  Stone200: 233,
  Stone300: 234,
  Stone400: 235,
  Stone500: 236,
  Stone600: 237,
  Stone700: 238,
  Stone800: 239,
  Stone900: 240,
  Stone950: 241,
  Taupe50: 242,
  Taupe100: 243,
  Taupe200: 244,
  Taupe300: 245,
  Taupe400: 246,
  Taupe500: 247,
  Taupe600: 248,
  Taupe700: 249,
  Taupe800: 250,
  Taupe900: 251,
  Taupe950: 252,
  Mauve50: 253,
  Mauve100: 254,
  Mauve200: 255,
  Mauve300: 256,
  Mauve400: 257,
  Mauve500: 258,
  Mauve600: 259,
  Mauve700: 260,
  Mauve800: 261,
  Mauve900: 262,
  Mauve950: 263,
  Mist50: 264,
  Mist100: 265,
  Mist200: 266,
  Mist300: 267,
  Mist400: 268,
  Mist500: 269,
  Mist600: 270,
  Mist700: 271,
  Mist800: 272,
  Mist900: 273,
  Mist950: 274,
  Olive50: 275,
  Olive100: 276,
  Olive200: 277,
  Olive300: 278,
  Olive400: 279,
  Olive500: 280,
  Olive600: 281,
  Olive700: 282,
  Olive800: 283,
  Olive900: 284,
  Olive950: 285,
  Black: 286,
  White: 287,
} as const;
// <end generated>

// A color from the kernel's fixed palette, as one of `Color`'s named
// entries (e.g. `Color.Slate900`). The underlying numeric id has no
// meaning of its own outside matching kernel/framebuffer/colors.rs's
// `Color` enum.
export type Color = (typeof Color)[keyof typeof Color];

// The kernel's set of built-in bitmap fonts.
// Generated from the font list in build/fonts.rs, so an entry's value is
// exactly the font id the kernel expects; kept in sync by hand the same way
// `Color` and `FRAMEBUFFER_WIDTH` are.
export const Font = {
  Cozette: 0,
} as const;

// One of `Font`'s named entries (e.g. `Font.Cozette`).
export type Font = (typeof Font)[keyof typeof Font];

export class DrawOutsideHandlerError extends Error {
  constructor() {
    super(
      "drawing calls can only be made from inside a registered draw handler",
    );
    this.name = "DrawOutsideHandlerError";
  }
}

// The framebuffer's logical resolution — kept in sync by hand with
// kernel/framebuffer.rs's `FRAMEBUFFER_WIDTH`/`FRAMEBUFFER_HEIGHT`, the same
// way `Color` above is kept in sync with colors.rs's `Color` enum.
const FRAMEBUFFER_WIDTH = 720;
const FRAMEBUFFER_HEIGHT = 360;

/** The framebuffer's logical width, in pixels. */
export function getWidth(): number {
  return FRAMEBUFFER_WIDTH;
}

/** The framebuffer's logical height, in pixels. */
export function getHeight(): number {
  return FRAMEBUFFER_HEIGHT;
}

/** The framebuffer's logical size, in pixels. */
export function getSize2d(): Size2d {
  return { width: FRAMEBUFFER_WIDTH, height: FRAMEBUFFER_HEIGHT };
}

let nextHandlerId = 1;
const drawHandlers = new Map<DrawTickerId, () => void>();
let insideDrawHandler = false;
let frameScheduled = false;

function frame() {
  frameScheduled = false;
  insideDrawHandler = true;
  try {
    for (const handler of [...drawHandlers.values()]) {
      try {
        handler();
      } finally {
        balanceStacks();
      }
    }
  } finally {
    insideDrawHandler = false;
  }
  if (drawHandlers.size > 0) scheduleFrame();
}

function scheduleFrame() {
  if (!frameScheduled) {
    frameScheduled = true;
    requestAnimationFrame(frame);
  }
}

/** Registers `handler` to run once per frame; `clearScreen`/`fillRectangle`
 * only take effect when called from inside a running handler. Returns an id
 * for `removeDrawHandler`. */
export function addDrawHandler(handler: () => void): DrawTickerId {
  const id = nextHandlerId++;
  drawHandlers.set(id, handler);
  scheduleFrame();
  return id;
}

/** Stops calling the draw handler registered under `id`. */
export function removeDrawHandler(id: DrawTickerId): void {
  drawHandlers.delete(id);
}

/** Clears the whole screen to `color`. */
export function clearScreen(color: Color): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_clear_screen(color);
}

/** Fills an axis-aligned rectangle at `(x, y)`, `w` wide and `h` tall, with `color`. */
export function fillRectangle(
  x: number,
  y: number,
  w: number,
  h: number,
  color: Color,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_fill_rectangle(x, y, w, h, color);
}

/** Which edge of the text box `drawText`'s `x` names. */
export type TextAlign = "left" | "center" | "right";

/** How `drawText` and `measureText` should lay a string out. */
export interface TextOptions {
  /** Which of the kernel's built-in fonts to use. */
  font?: Font;
  /** How many pixels wide to draw each of the font's own pixels — a whole
   * number of at least 1. A bigger size is the same bitmap with bigger
   * pixels, so it stays as crisp as the font itself. */
  scale?: number;
  /** Which edge of the text `x` names. Defaults to its left. */
  align?: TextAlign;
  /** Wraps the text to this width, breaking between words. A single word
   * too wide to fit still gets a line of its own and overruns it. */
  maxWidth?: number;
  /** Multiplies the gap between lines. */
  lineSpacing?: number;
}

interface ResolvedTextOptions {
  font: Font;
  scale: number;
  align: TextAlign;
  maxWidth: number | undefined;
  lineSpacing: number;
}

/** Accepts either a bare font, which is all `drawText` used to take, or the
 * full options object. */
function resolveTextOptions(
  fontOrOptions: Font | TextOptions | undefined,
): ResolvedTextOptions {
  const options =
    typeof fontOrOptions === "number"
      ? { font: fontOrOptions }
      : (fontOrOptions ?? {});
  const scale = options.scale ?? 1;
  if (!Number.isInteger(scale) || scale < 1) {
    throw new RangeError("text scale must be a whole number of at least 1");
  }
  return {
    font: options.font ?? Font.Cozette,
    scale,
    align: options.align ?? "left",
    maxWidth: options.maxWidth,
    lineSpacing: options.lineSpacing ?? 1,
  };
}

function lineWidth(line: string, options: ResolvedTextOptions): number {
  return __framebuffer_measure_text(line, options.font)[0] * options.scale;
}

/** Greedily packs as many words as fit within `maxWidth` onto each line. A
 * word wider than `maxWidth` on its own still gets its own line and
 * overruns it — there is no hyphenation or mid-word breaking. */
function wrapLine(line: string, options: ResolvedTextOptions): string[] {
  if (options.maxWidth === undefined) return [line];
  const wrapped: string[] = [];
  let current = "";
  for (const word of line.split(" ")) {
    const candidate = current === "" ? word : `${current} ${word}`;
    if (current !== "" && lineWidth(candidate, options) > options.maxWidth) {
      wrapped.push(current);
      current = word;
    } else {
      current = candidate;
    }
  }
  wrapped.push(current);
  return wrapped;
}

/** Where every line of `text` sits and how much room the whole block takes,
 * shared by `drawText` and `measureText` so the two can't disagree. */
function layoutText(text: string, options: ResolvedTextOptions) {
  const lines = text
    .split("\n")
    .flatMap((line) => wrapLine(line, options));
  const widths = lines.map((line) => lineWidth(line, options));
  const lineHeight = __framebuffer_measure_text("", options.font)[1] * options.scale;
  const step = lineHeight * options.lineSpacing;
  return {
    lines,
    widths,
    step,
    width: widths.reduce((widest, width) => Math.max(widest, width), 0),
    // The last line takes its full height rather than a spaced step, so a
    // single line measures exactly the font's line height whatever
    // `lineSpacing` says.
    height: (lines.length - 1) * step + lineHeight,
  };
}

/** Draws `text` in `color` with its top-left corner at `(x, y)`, using one
 * of the kernel's built-in bitmap fonts.
 *
 * Passing options instead of a bare font aligns the text against `x` rather
 * than starting from it, wraps it to a width, or draws it at a whole-number
 * multiple of the font's size. Line breaks in `text` are honoured either
 * way. Like the other draw calls, only takes effect from inside a running
 * draw handler. */
export function drawText(
  x: number,
  y: number,
  text: string,
  color: Color,
  fontOrOptions: Font | TextOptions = Font.Cozette,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  const options = resolveTextOptions(fontOrOptions);
  const { lines, widths, step } = layoutText(text, options);
  for (let i = 0; i < lines.length; i++) {
    let left = x;
    if (options.align === "center") left = x - widths[i] / 2;
    else if (options.align === "right") left = x - widths[i];
    __framebuffer_draw_text(
      left,
      y + step * i,
      lines[i],
      options.font,
      options.scale,
      color,
    );
  }
}

/** The pixel box `text` would occupy if drawn with the same options — the
 * width of its widest line and the height of the whole block. A query, not
 * a draw call: can be used from anywhere to lay text out without assuming
 * the font's size. */
export function measureText(
  text: string,
  fontOrOptions: Font | TextOptions = Font.Cozette,
): Size2d {
  const options = resolveTextOptions(fontOrOptions);
  const { width, height } = layoutText(text, options);
  return { width, height };
}

/** The palette entry closest to the RGB triplet `(r, g, b)` (each `0-255`). */
export function nearestColor(r: number, g: number, b: number): Color {
  return __framebuffer_nearest_color(r, g, b);
}

/** Sets how many physical pixels the window draws each logical pixel as —
 * an integer of at least 1. Takes effect on the next frame; unlike
 * `clearScreen`/`fillRectangle`, can be called from anywhere, not just
 * from inside a draw handler. */
export function setScale(scale: number): void {
  __framebuffer_set_scale(scale);
}

/** How a path decides which of its regions count as inside, where its
 * outline crosses over itself. `"nonzero"` counts a region inside when the
 * outline winds around it at all; `"evenodd"` alternates, so a shape drawn
 * inside another punches a hole in it. */
export type FillRule = "nonzero" | "evenodd";

/** How a stroke finishes at the two loose ends of an open path. */
export type LineCap = "butt" | "round" | "square";

/** How a stroke turns a corner where two segments meet. */
export type LineJoin = "miter" | "round" | "bevel";

export class UnbalancedStackError extends Error {
  constructor(what: string) {
    super(`popped ${what} that was never pushed`);
    this.name = "UnbalancedStackError";
  }
}

let transformDepth = 0;
let clipDepth = 0;

// Closes whatever the handler that just ran left open. A handler can return
// — or throw — without popping everything it pushed, and one frame is drawn
// by every handler of every running program in turn, so anything left open
// would go on confining or moving drawing that isn't the leaking program's
// at all.
function balanceStacks(): void {
  while (clipDepth > 0) {
    clipDepth--;
    __framebuffer_pop_clip();
  }
  while (transformDepth > 0) {
    transformDepth--;
    __framebuffer_pop_transform();
  }
}

/** Starts a new path, discarding whatever was being described before it.
 *
 * There is one path under construction at a time. The shape calls that
 * describe a whole path of their own — `fillCircle`, `pushClip` and the
 * rest — each start a new one, so they replace a path in progress rather
 * than adding to it. */
export function beginPath(): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
}

/** Starts a new contour of the current path at `(x, y)`, without drawing
 * anything on the way there. */
export function moveTo(x: number, y: number): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_move_to(x, y);
}

/** Extends the current path with a straight segment to `(x, y)`. */
export function lineTo(x: number, y: number): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_line_to(x, y);
}

/** Extends the current path with a curve to `(x, y)` that bends toward the
 * single control point `(cx, cy)` without passing through it. */
export function quadraticTo(
  cx: number,
  cy: number,
  x: number,
  y: number,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_quad_to(cx, cy, x, y);
}

/** Extends the current path with a curve to `(x, y)` that leaves along
 * `(c1x, c1y)` and arrives along `(c2x, c2y)` — the two-control-point curve
 * that can bend in an S. */
export function cubicTo(
  c1x: number,
  c1y: number,
  c2x: number,
  c2y: number,
  x: number,
  y: number,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_cubic_to(c1x, c1y, c2x, c2y, x, y);
}

/** Closes the current contour with a straight segment back to where it
 * started. A path is filled as though every contour were closed, so this
 * matters to `strokePath`, which would otherwise leave the loop open. */
export function closePath(): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_close();
}

/** Fills the inside of the current path with `color`. Leaves the path in
 * place, so it can be stroked afterwards without describing it again. */
export function fillPath(color: Color, rule: FillRule = "nonzero"): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_fill_path(color, rule);
}

/** Draws a line of `thickness` along the current path in `color`. The line
 * straddles the path, half its thickness to either side. Leaves the path in
 * place. */
export function strokePath(
  color: Color,
  thickness: number = 1,
  cap: LineCap = "butt",
  join: LineJoin = "miter",
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_stroke_path(color, thickness, cap, join);
}

/** How `pushTransform` should move the coordinate space. Applied in the
 * order written: a shape is scaled, then rotated, then shifted. */
export interface Transform {
  /** Shifts by this much, in the coordinates outside the transform. */
  translate?: Vector2d;
  /** Scales about the origin. A single number scales both axes alike. */
  scale?: Vector2d | number;
  /** Turns about the origin, in radians — clockwise on screen, since `y`
   * grows downward. */
  rotate?: number;
}

/** Moves the coordinate space everything drawn afterwards is placed in,
 * until the matching `popTransform`. Transforms nest: pushing a second one
 * applies inside the first rather than replacing it. */
export function pushTransform(transform: Transform): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  const { translate, scale, rotate = 0 } = transform;
  const sx = typeof scale === "number" ? scale : (scale?.x ?? 1);
  const sy = typeof scale === "number" ? scale : (scale?.y ?? 1);
  const cos = Math.cos(rotate);
  const sin = Math.sin(rotate);
  transformDepth++;
  // The 2x3 matrix for shift * turn * scale, column by column.
  __framebuffer_push_transform(
    cos * sx,
    sin * sx,
    -sin * sy,
    cos * sy,
    translate?.x ?? 0,
    translate?.y ?? 0,
  );
}

/** Restores the coordinate space in effect before the matching
 * `pushTransform`. */
export function popTransform(): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  if (transformDepth === 0) throw new UnbalancedStackError("a transform");
  transformDepth--;
  __framebuffer_pop_transform();
}

/** Confines everything drawn afterwards to the rectangle at `(x, y)`, until
 * the matching `popClip`. Clips nest by narrowing: drawing can never escape
 * a region an enclosing clip already confined it to. Starts a new path. */
export function pushClip(x: number, y: number, w: number, h: number): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_rect(x, y, w, h);
  clipDepth++;
  __framebuffer_push_clip("nonzero");
}

/** Confines everything drawn afterwards to the inside of the current path,
 * until the matching `popClip` — the arbitrary-shape form of `pushClip`. */
export function pushClipPath(rule: FillRule = "nonzero"): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  clipDepth++;
  __framebuffer_push_clip(rule);
}

/** Restores the region in effect before the matching `pushClip`. */
export function popClip(): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  if (clipDepth === 0) throw new UnbalancedStackError("a clip");
  clipDepth--;
  __framebuffer_pop_clip();
}

// The shapes below each describe a whole path of their own, so they all
// start a new one and replace whatever was being built. They go through the
// same fill and stroke the raw path calls do; nothing here is a special case
// in the kernel.

/** Draws the outline of an axis-aligned rectangle at `(x, y)`, `w` wide and
 * `h` tall. The outline straddles the rectangle's edge, half its thickness
 * to either side, so it doesn't cover exactly the same pixels `fillRectangle`
 * would. */
export function strokeRectangle(
  x: number,
  y: number,
  w: number,
  h: number,
  color: Color,
  thickness: number = 1,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_rect(x, y, w, h);
  __framebuffer_stroke_path(color, thickness, "butt", "miter");
}

/** Fills a rectangle at `(x, y)` whose corners are rounded off by `radius`,
 * clamped to half the shorter side. */
export function fillRoundedRectangle(
  x: number,
  y: number,
  w: number,
  h: number,
  radius: number,
  color: Color,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_rounded_rect(x, y, w, h, radius);
  __framebuffer_fill_path(color, "nonzero");
}

/** Draws the outline of a rectangle at `(x, y)` with corners rounded off by
 * `radius`. */
export function strokeRoundedRectangle(
  x: number,
  y: number,
  w: number,
  h: number,
  radius: number,
  color: Color,
  thickness: number = 1,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_rounded_rect(x, y, w, h, radius);
  __framebuffer_stroke_path(color, thickness, "butt", "miter");
}

/** Draws a straight line from `(x1, y1)` to `(x2, y2)`.
 *
 * A line straddles the coordinates it runs along, so a thickness of 1 down a
 * whole coordinate covers half of each neighbouring pixel column. Run it down
 * the middle of a column — `x + 0.5` — for one crisp line. */
export function drawLine(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  color: Color,
  thickness: number = 1,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_move_to(x1, y1);
  __framebuffer_path_line_to(x2, y2);
  __framebuffer_stroke_path(color, thickness, "butt", "miter");
}

/** Draws straight lines through `points` in order, leaving the two ends
 * loose. Fewer than two points draws nothing. */
export function drawPolyline(
  points: readonly Vector2d[],
  color: Color,
  thickness: number = 1,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  if (points.length < 2) return;
  tracePoints(points);
  __framebuffer_stroke_path(color, thickness, "butt", "round");
}

/** Fills a circle of radius `r` centred on `(cx, cy)`. */
export function fillCircle(
  cx: number,
  cy: number,
  r: number,
  color: Color,
): void {
  fillEllipse(cx, cy, r, r, color);
}

/** Draws the outline of a circle of radius `r` centred on `(cx, cy)`. The
 * outline straddles the radius, half its thickness to either side. */
export function strokeCircle(
  cx: number,
  cy: number,
  r: number,
  color: Color,
  thickness: number = 1,
): void {
  strokeEllipse(cx, cy, r, r, color, thickness);
}

/** Fills an axis-aligned ellipse centred on `(cx, cy)`, reaching `rx` to
 * either side and `ry` above and below. */
export function fillEllipse(
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  color: Color,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_oval(cx, cy, rx, ry);
  __framebuffer_fill_path(color, "nonzero");
}

/** Draws the outline of an axis-aligned ellipse centred on `(cx, cy)`. */
export function strokeEllipse(
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  color: Color,
  thickness: number = 1,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_oval(cx, cy, rx, ry);
  __framebuffer_stroke_path(color, thickness, "butt", "miter");
}

/** Draws the piece of a circle's rim running from `startRad` to `endRad`,
 * in radians measured from `+x` and increasing clockwise on screen. The
 * sweep follows which way round the two angles are named, so naming them
 * backwards sweeps the other way — which is how a meter winds down. */
export function drawArc(
  cx: number,
  cy: number,
  r: number,
  startRad: number,
  endRad: number,
  color: Color,
  thickness: number = 1,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_path_begin();
  __framebuffer_path_arc(cx, cy, r, startRad, endRad);
  __framebuffer_stroke_path(color, thickness, "butt", "round");
}

/** Fills the triangle with corners `a`, `b` and `c`. */
export function fillTriangle(
  a: Vector2d,
  b: Vector2d,
  c: Vector2d,
  color: Color,
): void {
  fillPolygon([a, b, c], color);
}

/** Fills the shape enclosed by `points`, joined in order and closed back to
 * the first. Fewer than three points encloses nothing and draws nothing.
 *
 * The outline may cross over itself; `rule` decides what counts as inside
 * where it does. */
export function fillPolygon(
  points: readonly Vector2d[],
  color: Color,
  rule: FillRule = "nonzero",
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  if (points.length < 3) return;
  tracePoints(points);
  __framebuffer_path_close();
  __framebuffer_fill_path(color, rule);
}

/** Draws the outline of the shape enclosed by `points`, closed back to the
 * first — unlike `drawPolyline`, which leaves its ends loose. */
export function strokePolygon(
  points: readonly Vector2d[],
  color: Color,
  thickness: number = 1,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  if (points.length < 3) return;
  tracePoints(points);
  __framebuffer_path_close();
  __framebuffer_stroke_path(color, thickness, "butt", "miter");
}

/** Starts a fresh path running through `points` in order, leaving it open
 * for the caller to close, fill or stroke. */
function tracePoints(points: readonly Vector2d[]): void {
  __framebuffer_path_begin();
  __framebuffer_path_move_to(points[0].x, points[0].y);
  for (let i = 1; i < points.length; i++) {
    __framebuffer_path_line_to(points[i].x, points[i].y);
  }
}

/** Sets the single pixel that `(x, y)` falls inside. Coordinates name the
 * corners of the pixel grid, so `(3, 4)` and `(3.5, 4.5)` both set the same
 * pixel — the fourth across and fifth down. */
export function setPixel(x: number, y: number, color: Color): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  __framebuffer_set_pixel(x, y, color);
}

/** Sets every pixel in `points` to the same color. */
export function drawPixels(points: readonly Vector2d[], color: Color): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  for (const point of points) {
    __framebuffer_set_pixel(point.x, point.y, color);
  }
}

/** A 2x3 matrix laid out so that a point `(x, y)` maps to
 * `(a x + c y + e, b x + d y + f)` — the same six numbers the kernel takes. */
type Matrix = [number, number, number, number, number, number];

/** `outer` applied after `inner`, so `inner` acts on a point first. */
function concat(outer: Matrix, inner: Matrix): Matrix {
  const [a, b, c, d, e, f] = outer;
  const [g, h, i, j, k, l] = inner;
  return [
    a * g + c * h,
    b * g + d * h,
    a * i + c * j,
    b * i + d * j,
    a * k + c * l + e,
    b * k + d * l + f,
  ];
}

/** Which part of an image to draw, and how to place it. */
export interface DrawImageOptions {
  /** The left edge of the part of the image to draw. Defaults to 0. */
  sx?: number;
  /** The top edge of the part of the image to draw. Defaults to 0. */
  sy?: number;
  /** The width of the part of the image to draw. Defaults to the rest of
   * it, to the right of `sx`. */
  sw?: number;
  /** The height of the part of the image to draw. Defaults to the rest of
   * it, below `sy`. */
  sh?: number;
  /** Draws the image this many times its natural size. A single number
   * scales both axes alike. Whole numbers keep it pixel-crisp; anything
   * else lands its pixels unevenly, since nothing is smoothed. */
  scale?: number | Vector2d;
  /** Mirrors the image left to right, within the same destination box. */
  flipX?: boolean;
  /** Mirrors the image top to bottom, within the same destination box. */
  flipY?: boolean;
}

/** Where an image turns about, in the drawn image's own pixels, measured
 * from its top-left corner. */
export interface DrawImageRotatedOptions extends DrawImageOptions {
  /** Defaults to the left edge. */
  originX?: number;
  /** Defaults to the top edge. */
  originY?: number;
}

function imageId(image: Image | ImageId): number {
  return typeof image === "number" ? image : image.id;
}

/** The source rect asked for, filled in against the image's real size. */
function sourceRect(
  id: number,
  options: DrawImageOptions,
): [number, number, number, number] {
  const sx = options.sx ?? 0;
  const sy = options.sy ?? 0;
  return [
    sx,
    sy,
    options.sw ?? __image_width(id) - sx,
    options.sh ?? __image_height(id) - sy,
  ];
}

/** Maps the source rect's own box onto the surface: mirrored, resized,
 * turned about its origin, and finally moved to `(x, y)`. */
function placement(
  x: number,
  y: number,
  sw: number,
  sh: number,
  radians: number,
  options: DrawImageRotatedOptions,
): Matrix {
  const scale = options.scale ?? 1;
  const scaleX = typeof scale === "number" ? scale : scale.x;
  const scaleY = typeof scale === "number" ? scale : scale.y;

  // Mirroring folds the box back onto itself, so a flipped image covers the
  // same destination as an unflipped one.
  let matrix: Matrix = [
    options.flipX ? -1 : 1,
    0,
    0,
    options.flipY ? -1 : 1,
    options.flipX ? sw : 0,
    options.flipY ? sh : 0,
  ];
  matrix = concat([scaleX, 0, 0, scaleY, 0, 0], matrix);

  if (radians !== 0) {
    const ox = options.originX ?? 0;
    const oy = options.originY ?? 0;
    const cos = Math.cos(radians);
    const sin = Math.sin(radians);
    // Turn about the origin: bring it to zero, turn, and put it back.
    matrix = concat(
      concat([1, 0, 0, 1, ox, oy], [cos, sin, -sin, cos, 0, 0]),
      concat([1, 0, 0, 1, -ox, -oy], matrix),
    );
  }

  return concat([1, 0, 0, 1, x, y], matrix);
}

/** Draws `image` with its top-left corner at `(x, y)`.
 *
 * With no options it goes on at its natural size, whole. Options take part
 * of it instead, resize it, or mirror it — see `DrawImageOptions`. */
export function drawImage(
  image: Image | ImageId,
  x: number,
  y: number,
  options?: DrawImageOptions,
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  const id = imageId(image);
  if (options === undefined) {
    __framebuffer_draw_image(id, x, y);
    return;
  }
  const source = sourceRect(id, options);
  __framebuffer_draw_image_transformed(
    id,
    source,
    placement(x, y, source[2], source[3], 0, options),
  );
}

/** Draws `image` at `(x, y)`, turned `radians` about the point `originX`,
 * `originY` within it — clockwise on screen, since `y` grows downward. The
 * origin defaults to the image's top-left corner, so turning about its
 * middle means naming its middle. */
export function drawImageRotated(
  image: Image | ImageId,
  x: number,
  y: number,
  radians: number,
  options: DrawImageRotatedOptions = {},
): void {
  if (!insideDrawHandler) throw new DrawOutsideHandlerError();
  const id = imageId(image);
  const source = sourceRect(id, options);
  __framebuffer_draw_image_transformed(
    id,
    source,
    placement(x, y, source[2], source[3], radians, options),
  );
}
