// The drawing surface. A `Surface` is a retained image render operations
// apply to immediately and stay; the screen is one, and a program can make
// more with `createSurface`. Colors are always one of `Color`'s named
// entries (the kernel's fixed palette), never raw RGBA channels a program
// could get wrong.

import type { Size2d, Vector2d } from "ely:math";
import type { Image, ImageId } from "ely:image";

// The surface lifecycle. `__surface_use` reports whether the handle names a
// live surface; the rest throw on a dead handle.
declare function __surface_create(width: number, height: number): number;
declare function __surface_use(handle: number): boolean;
declare function __surface_destroy(handle: number): void;
declare function __surface_dimensions(handle: number): [number, number];
declare function __surface_resize(
  handle: number,
  width: number,
  height: number,
): void;

// Drawing, each onto the surface the leading handle names.
declare function __surface_clear(handle: number, color: Color): void;
declare function __surface_fill_rectangle(
  handle: number,
  x: number,
  y: number,
  w: number,
  h: number,
  color: Color,
): void;
declare function __surface_set_pixel(
  handle: number,
  x: number,
  y: number,
  color: Color,
): void;
declare function __surface_draw_text(
  handle: number,
  x: number,
  y: number,
  text: string,
  color: Color,
  options: NormalizedTextOptions,
): void;
declare function __surface_measure_text(
  text: string,
  options: NormalizedTextOptions,
): [number, number];
declare function __surface_draw_image(
  handle: number,
  image: number,
  x: number,
  y: number,
): void;
declare function __surface_draw_image_transformed(
  handle: number,
  image: number,
  source: [number, number, number, number],
  transform: [number, number, number, number, number, number],
): void;
declare function __surface_draw_surface(
  handle: number,
  source: number,
  x: number,
  y: number,
): void;
declare function __surface_draw_surface_transformed(
  handle: number,
  source: number,
  region: [number, number, number, number],
  transform: [number, number, number, number, number, number],
): void;
// `{ translate, scale, rotate }` reach the kernel as those three parts; the
// 2x3 for shift * turn * scale is composed there.
declare function __surface_push_transform(
  handle: number,
  translate: [number, number],
  scale: [number, number],
  rotate: number,
): void;
declare function __surface_pop_transform(handle: number): void;

// The shape vocabulary. Geometry with more than three numbers is passed as
// one array; the kernel lowers each shape onto a path fill or stroke.
declare function __surface_stroke_rectangle(
  handle: number,
  rect: [number, number, number, number],
  color: Color,
  thickness: number,
): void;
declare function __surface_fill_rounded_rectangle(
  handle: number,
  rect: [number, number, number, number],
  radius: number,
  color: Color,
): void;
declare function __surface_stroke_rounded_rectangle(
  handle: number,
  rectRadius: [number, number, number, number, number],
  color: Color,
  thickness: number,
): void;
declare function __surface_draw_line(
  handle: number,
  ends: [number, number, number, number],
  color: Color,
  thickness: number,
): void;
declare function __surface_draw_polyline(
  handle: number,
  points: number[],
  color: Color,
  thickness: number,
): void;
declare function __surface_fill_ellipse(
  handle: number,
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  color: Color,
): void;
declare function __surface_stroke_ellipse(
  handle: number,
  oval: [number, number, number, number],
  color: Color,
  thickness: number,
): void;
declare function __surface_draw_arc(
  handle: number,
  arc: [number, number, number, number, number],
  color: Color,
  thickness: number,
): void;
declare function __surface_fill_polygon(
  handle: number,
  points: number[],
  color: Color,
  rule: FillRule,
): void;
declare function __surface_stroke_polygon(
  handle: number,
  points: number[],
  color: Color,
  thickness: number,
): void;

// The path pen and clip stack. The pen is one per program, not one per
// surface; only fill/stroke/clip carry a surface handle.
declare function __surface_path_begin(): void;
declare function __surface_path_move_to(x: number, y: number): void;
declare function __surface_path_line_to(x: number, y: number): void;
declare function __surface_path_quad_to(
  cx: number,
  cy: number,
  x: number,
  y: number,
): void;
declare function __surface_path_cubic_to(
  c1x: number,
  c1y: number,
  c2x: number,
  c2y: number,
  x: number,
  y: number,
): void;
declare function __surface_path_close(): void;
declare function __surface_fill_path(
  handle: number,
  color: Color,
  rule: FillRule,
): void;
declare function __surface_stroke_path(
  handle: number,
  color: Color,
  thickness: number,
  cap: LineCap,
  join: LineJoin,
): void;
declare function __surface_push_clip_rect(
  handle: number,
  x: number,
  y: number,
  w: number,
  h: number,
): void;
declare function __surface_push_clip(handle: number, rule: FillRule): void;
declare function __surface_pop_clip(handle: number): void;

declare function __surface_nearest_color(
  r: number,
  g: number,
  b: number,
): Color;
declare function __surface_set_scale(scale: number): void;
declare function __image_width(id: number): number;
declare function __image_height(id: number): number;

// The kernel's fixed, curated color palette. Every color a program can
// draw with is one of these named entries — never a raw, unconstrained
// RGBA value.
//
// <generated from kernel/graphics/palette.rs>
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
// meaning of its own outside matching kernel/graphics/colors.rs's
// `Color` enum.
export type Color = (typeof Color)[keyof typeof Color];

// The kernel's set of built-in bitmap fonts.
// Generated from the font list in build/fonts.rs, so an entry's value is
// exactly the font id the kernel expects; kept in sync by hand the same way
// `Color` is.
export const Font = {
  Cozette: 0,
} as const;

// One of `Font`'s named entries (e.g. `Font.Cozette`).
export type Font = (typeof Font)[keyof typeof Font];

/** A number naming a surface, valid across every process. Send it over
 * process IPC as an ordinary number; the receiver revives it with
 * `useSurface`. */
export type SurfaceHandle = number;

/** Which edge of the text box `drawText`'s `x` names. */
export type TextAlign = "left" | "center" | "right";

/** How `drawText` and `measureText` should lay a string out. */
export interface TextOptions {
  /** Which of the kernel's built-in fonts to use. */
  font?: Font;
  /** How many pixels wide to draw each of the font's own pixels — a whole
   * number of at least 1. */
  scale?: number;
  /** Which edge of the text `x` names. Defaults to its left. */
  align?: TextAlign;
  /** Wraps the text to this width, breaking between words. */
  maxWidth?: number;
  /** Multiplies the gap between lines. */
  lineSpacing?: number;
}

/** `TextOptions` with defaults filled in, the shape the kernel reads. */
interface NormalizedTextOptions {
  font: Font;
  scale: number;
  align: TextAlign;
  maxWidth?: number;
  lineSpacing: number;
}

/** How a path decides which of its regions count as inside, where its
 * outline crosses over itself. */
export type FillRule = "nonzero" | "evenodd";

/** How a stroke finishes at the two loose ends of an open path. */
export type LineCap = "butt" | "round" | "square";

/** How a stroke turns a corner where two segments meet. */
export type LineJoin = "miter" | "round" | "bevel";

/** How `pushTransform` should move the coordinate space. Applied in the
 * order written: a shape is scaled, then rotated, then shifted. */
export interface Transform {
  /** Shifts by this much, in the coordinates outside the transform. */
  translate?: Vector2d;
  /** Scales about the origin. A single number scales both axes alike. */
  scale?: Vector2d | number;
  /** Turns about the origin, in radians — clockwise on screen. */
  rotate?: number;
}

/** Which part of an image to draw, and how to place it. */
export interface DrawImageOptions {
  /** The left edge of the part to draw. Defaults to 0. */
  sx?: number;
  /** The top edge of the part to draw. Defaults to 0. */
  sy?: number;
  /** The width of the part to draw. Defaults to the rest, right of `sx`. */
  sw?: number;
  /** The height of the part to draw. Defaults to the rest, below `sy`. */
  sh?: number;
  /** Draws it this many times its natural size. Whole numbers stay crisp. */
  scale?: number | Vector2d;
  /** Mirrors left to right, within the same destination box. */
  flipX?: boolean;
  /** Mirrors top to bottom, within the same destination box. */
  flipY?: boolean;
}

/** Where an image or surface turns about, in the drawn copy's own pixels,
 * measured from its top-left corner. */
export interface DrawImageRotatedOptions extends DrawImageOptions {
  /** Defaults to the left edge. */
  originX?: number;
  /** Defaults to the top edge. */
  originY?: number;
}

const SCREEN_HANDLE: SurfaceHandle = 0;

/** A 2x3 matrix laid out so that `(x, y)` maps to
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

/** Accepts either a bare font, which is all `drawText` used to take, or the
 * full options object, and fills in the defaults the kernel expects. */
function normalizeTextOptions(
  fontOrOptions: Font | TextOptions | undefined,
): NormalizedTextOptions {
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

/** Flattens points into the `[x0, y0, x1, y1, ...]` a shape binding takes. */
function flatten(points: readonly Vector2d[]): number[] {
  const flat: number[] = [];
  for (const p of points) {
    flat.push(p.x, p.y);
  }
  return flat;
}

/** The source rect asked for, filled in against a full `w` x `h`. */
function croppedRect(
  w: number,
  h: number,
  options: DrawImageOptions,
): [number, number, number, number] {
  const sx = options.sx ?? 0;
  const sy = options.sy ?? 0;
  return [sx, sy, options.sw ?? w - sx, options.sh ?? h - sy];
}

/** Maps a source rect's own box onto the surface: mirrored, resized,
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
    matrix = concat(
      concat([1, 0, 0, 1, ox, oy], [cos, sin, -sin, cos, 0, 0]),
      concat([1, 0, 0, 1, -ox, -oy], matrix),
    );
  }

  return concat([1, 0, 0, 1, x, y], matrix);
}

function imageId(image: Image | ImageId): number {
  return typeof image === "number" ? image : image.id;
}

/** A retained drawing surface. Every operation applies to its pixels right
 * away and stays; the transform and clip stacks persist between calls.
 *
 * Get one with `useSurface(handle)` — for the screen, the exported `screen`.
 * `createSurface` returns a bare handle, the thing you send to another
 * process. */
export class Surface {
  #handle: SurfaceHandle;

  constructor(handle: SurfaceHandle) {
    this.#handle = handle;
  }

  /** The number to send over process IPC. */
  get handle(): SurfaceHandle {
    return this.#handle;
  }

  /** Live — reflects a resize by any holder of the handle. */
  get width(): number {
    return __surface_dimensions(this.#handle)[0];
  }

  get height(): number {
    return __surface_dimensions(this.#handle)[1];
  }

  get size(): Size2d {
    const [width, height] = __surface_dimensions(this.#handle);
    return { width, height };
  }

  /** Resizes the surface. Contents keep their top-left corner; newly
   * exposed area is transparent. Empties the transform and clip stacks. */
  resize(width: number, height: number): void {
    __surface_resize(this.#handle, width, height);
  }

  /** Fills the whole surface with `color`, discarding whatever it held. */
  clear(color: Color): void {
    __surface_clear(this.#handle, color);
  }

  /** Fills an axis-aligned rectangle at `(x, y)`, `w` wide and `h` tall. */
  fillRectangle(x: number, y: number, w: number, h: number, color: Color): void {
    __surface_fill_rectangle(this.#handle, x, y, w, h, color);
  }

  /** Draws the outline of an axis-aligned rectangle. The outline straddles
   * the edge, so it doesn't land on the same pixels `fillRectangle` would. */
  strokeRectangle(
    x: number,
    y: number,
    w: number,
    h: number,
    color: Color,
    thickness: number = 1,
  ): void {
    __surface_stroke_rectangle(this.#handle, [x, y, w, h], color, thickness);
  }

  /** Fills a rectangle whose corners are rounded off by `radius`, clamped
   * to half the shorter side. */
  fillRoundedRectangle(
    x: number,
    y: number,
    w: number,
    h: number,
    radius: number,
    color: Color,
  ): void {
    __surface_fill_rounded_rectangle(this.#handle, [x, y, w, h], radius, color);
  }

  /** Draws the outline of a rounded rectangle. */
  strokeRoundedRectangle(
    x: number,
    y: number,
    w: number,
    h: number,
    radius: number,
    color: Color,
    thickness: number = 1,
  ): void {
    __surface_stroke_rounded_rectangle(
      this.#handle,
      [x, y, w, h, radius],
      color,
      thickness,
    );
  }

  /** Draws a straight line from `(x1, y1)` to `(x2, y2)`. Run it down the
   * middle of a column — `x + 0.5` — for one crisp line. */
  drawLine(
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    color: Color,
    thickness: number = 1,
  ): void {
    __surface_draw_line(this.#handle, [x1, y1, x2, y2], color, thickness);
  }

  /** Draws straight lines through `points` in order, ends left loose. */
  drawPolyline(
    points: readonly Vector2d[],
    color: Color,
    thickness: number = 1,
  ): void {
    if (points.length < 2) return;
    __surface_draw_polyline(this.#handle, flatten(points), color, thickness);
  }

  /** Fills a circle of radius `r` centred on `(cx, cy)`. */
  fillCircle(cx: number, cy: number, r: number, color: Color): void {
    this.fillEllipse(cx, cy, r, r, color);
  }

  /** Draws the outline of a circle. */
  strokeCircle(
    cx: number,
    cy: number,
    r: number,
    color: Color,
    thickness: number = 1,
  ): void {
    this.strokeEllipse(cx, cy, r, r, color, thickness);
  }

  /** Fills an axis-aligned ellipse centred on `(cx, cy)`. */
  fillEllipse(
    cx: number,
    cy: number,
    rx: number,
    ry: number,
    color: Color,
  ): void {
    __surface_fill_ellipse(this.#handle, cx, cy, rx, ry, color);
  }

  /** Draws the outline of an axis-aligned ellipse. */
  strokeEllipse(
    cx: number,
    cy: number,
    rx: number,
    ry: number,
    color: Color,
    thickness: number = 1,
  ): void {
    __surface_stroke_ellipse(this.#handle, [cx, cy, rx, ry], color, thickness);
  }

  /** Draws the piece of a circle's rim from `startRad` to `endRad`. Naming
   * the angles backwards sweeps the other way. */
  drawArc(
    cx: number,
    cy: number,
    r: number,
    startRad: number,
    endRad: number,
    color: Color,
    thickness: number = 1,
  ): void {
    __surface_draw_arc(
      this.#handle,
      [cx, cy, r, startRad, endRad],
      color,
      thickness,
    );
  }

  /** Fills the triangle with corners `a`, `b` and `c`. */
  fillTriangle(a: Vector2d, b: Vector2d, c: Vector2d, color: Color): void {
    this.fillPolygon([a, b, c], color);
  }

  /** Fills the shape enclosed by `points`, closed back to the first. Fewer
   * than three points draws nothing. */
  fillPolygon(
    points: readonly Vector2d[],
    color: Color,
    rule: FillRule = "nonzero",
  ): void {
    if (points.length < 3) return;
    __surface_fill_polygon(this.#handle, flatten(points), color, rule);
  }

  /** Draws the outline of the shape enclosed by `points`, closed back to
   * the first. */
  strokePolygon(
    points: readonly Vector2d[],
    color: Color,
    thickness: number = 1,
  ): void {
    if (points.length < 3) return;
    __surface_stroke_polygon(this.#handle, flatten(points), color, thickness);
  }

  /** Sets the single pixel `(x, y)` falls inside. */
  setPixel(x: number, y: number, color: Color): void {
    __surface_set_pixel(this.#handle, x, y, color);
  }

  /** Sets every pixel in `points` to the same color. */
  drawPixels(points: readonly Vector2d[], color: Color): void {
    for (const point of points) {
      __surface_set_pixel(this.#handle, point.x, point.y, color);
    }
  }

  /** Starts a new path, discarding whatever was being described. There is
   * one path under construction per program, shared across surfaces. */
  beginPath(): void {
    __surface_path_begin();
  }

  /** Starts a new contour at `(x, y)` without drawing on the way there. */
  moveTo(x: number, y: number): void {
    __surface_path_move_to(x, y);
  }

  /** Extends the current path with a straight segment to `(x, y)`. */
  lineTo(x: number, y: number): void {
    __surface_path_line_to(x, y);
  }

  /** Extends the current path with a curve to `(x, y)` bending toward
   * `(cx, cy)` without passing through it. */
  quadraticTo(cx: number, cy: number, x: number, y: number): void {
    __surface_path_quad_to(cx, cy, x, y);
  }

  /** Extends the current path with a curve to `(x, y)` leaving along
   * `(c1x, c1y)` and arriving along `(c2x, c2y)`. */
  cubicTo(
    c1x: number,
    c1y: number,
    c2x: number,
    c2y: number,
    x: number,
    y: number,
  ): void {
    __surface_path_cubic_to(c1x, c1y, c2x, c2y, x, y);
  }

  /** Closes the current contour with a straight segment back to its start. */
  closePath(): void {
    __surface_path_close();
  }

  /** Fills the inside of the current path. Leaves the path in place. */
  fillPath(color: Color, rule: FillRule = "nonzero"): void {
    __surface_fill_path(this.#handle, color, rule);
  }

  /** Draws a line of `thickness` along the current path. Leaves it in
   * place. */
  strokePath(
    color: Color,
    thickness: number = 1,
    cap: LineCap = "butt",
    join: LineJoin = "miter",
  ): void {
    __surface_stroke_path(this.#handle, color, thickness, cap, join);
  }

  /** Moves the coordinate space everything drawn afterwards is placed in,
   * until the matching `popTransform`. Transforms nest. */
  pushTransform(transform: Transform): void {
    const { translate, scale, rotate = 0 } = transform;
    const sx = typeof scale === "number" ? scale : (scale?.x ?? 1);
    const sy = typeof scale === "number" ? scale : (scale?.y ?? 1);
    __surface_push_transform(
      this.#handle,
      [translate?.x ?? 0, translate?.y ?? 0],
      [sx, sy],
      rotate,
    );
  }

  /** Restores the coordinate space from before the matching
   * `pushTransform`. */
  popTransform(): void {
    __surface_pop_transform(this.#handle);
  }

  /** Confines everything drawn afterwards to the rectangle at `(x, y)`,
   * until the matching `popClip`. Clips nest by narrowing. */
  pushClip(x: number, y: number, w: number, h: number): void {
    __surface_push_clip_rect(this.#handle, x, y, w, h);
  }

  /** Confines everything drawn afterwards to the inside of the current
   * path — the arbitrary-shape form of `pushClip`. */
  pushClipPath(rule: FillRule = "nonzero"): void {
    __surface_push_clip(this.#handle, rule);
  }

  /** Restores the region from before the matching `pushClip`. */
  popClip(): void {
    __surface_pop_clip(this.#handle);
  }

  /** Draws `text` in `color` with its top-left corner at `(x, y)`. Passing
   * options instead of a bare font aligns, wraps, or scales it. */
  drawText(
    x: number,
    y: number,
    text: string,
    color: Color,
    fontOrOptions: Font | TextOptions = Font.Cozette,
  ): void {
    __surface_draw_text(
      this.#handle,
      x,
      y,
      text,
      color,
      normalizeTextOptions(fontOrOptions),
    );
  }

  /** The pixel box `text` would occupy — a query, not a draw call. */
  measureText(
    text: string,
    fontOrOptions: Font | TextOptions = Font.Cozette,
  ): Size2d {
    const [width, height] = __surface_measure_text(
      text,
      normalizeTextOptions(fontOrOptions),
    );
    return { width, height };
  }

  /** Draws `image` with its top-left corner at `(x, y)`. Options take part
   * of it, resize it, or mirror it. */
  drawImage(
    image: Image | ImageId,
    x: number,
    y: number,
    options?: DrawImageOptions,
  ): void {
    const id = imageId(image);
    if (options === undefined) {
      __surface_draw_image(this.#handle, id, x, y);
      return;
    }
    const source = croppedRect(__image_width(id), __image_height(id), options);
    __surface_draw_image_transformed(
      this.#handle,
      id,
      source,
      placement(x, y, source[2], source[3], 0, options),
    );
  }

  /** Draws `image` at `(x, y)`, turned `radians` about `(originX, originY)`
   * within it — the origin defaulting to its top-left corner. */
  drawImageRotated(
    image: Image | ImageId,
    x: number,
    y: number,
    radians: number,
    options: DrawImageRotatedOptions = {},
  ): void {
    const id = imageId(image);
    const source = croppedRect(__image_width(id), __image_height(id), options);
    __surface_draw_image_transformed(
      this.#handle,
      id,
      source,
      placement(x, y, source[2], source[3], radians, options),
    );
  }

  /** Draws another surface onto this one, like an image, with its top-left
   * corner at `(x, y)`. A surface can't be drawn onto itself. */
  drawSurface(
    source: SurfaceHandle | Surface,
    x: number,
    y: number,
    options?: DrawImageOptions,
  ): void {
    const src = typeof source === "number" ? source : source.handle;
    if (options === undefined) {
      __surface_draw_surface(this.#handle, src, x, y);
      return;
    }
    const [w, h] = __surface_dimensions(src);
    const region = croppedRect(w, h, options);
    __surface_draw_surface_transformed(
      this.#handle,
      src,
      region,
      placement(x, y, region[2], region[3], 0, options),
    );
  }

  /** Draws another surface onto this one, turned `radians` about
   * `(originX, originY)` within it. */
  drawSurfaceRotated(
    source: SurfaceHandle | Surface,
    x: number,
    y: number,
    radians: number,
    options: DrawImageRotatedOptions = {},
  ): void {
    const src = typeof source === "number" ? source : source.handle;
    const [w, h] = __surface_dimensions(src);
    const region = croppedRect(w, h, options);
    __surface_draw_surface_transformed(
      this.#handle,
      src,
      region,
      placement(x, y, region[2], region[3], radians, options),
    );
  }
}

/** The screen — the surface the kernel presents at the end of every tick.
 * Draw to it directly; nothing else needs to happen first. */
export const screen: Surface = new Surface(SCREEN_HANDLE);

/** Makes a new, fully transparent surface and returns its handle — the
 * thing to send to another process. Revive it into something drawable with
 * `useSurface`. */
export function createSurface(width: number, height: number): SurfaceHandle {
  return __surface_create(width, height);
}

/** Revives a handle — yours, or one another process sent you — into a
 * `Surface` you can draw with. Throws if the handle names no live
 * surface. */
export function useSurface(handle: SurfaceHandle): Surface {
  if (!__surface_use(handle)) {
    throw new TypeError(`${handle} is not a live surface`);
  }
  return new Surface(handle);
}

/** Drops this process's hold on `handle`. The surface goes away once no
 * live process holds it. */
export function destroySurface(handle: SurfaceHandle): void {
  __surface_destroy(handle);
}

/** The palette entry closest to the RGB triplet `(r, g, b)` (each `0-255`). */
export function nearestColor(r: number, g: number, b: number): Color {
  return __surface_nearest_color(r, g, b);
}

/** Sets how many physical pixels the window draws each logical pixel as —
 * an integer of at least 1. Takes effect on the next frame; can be called
 * from anywhere. One window shared by every process, last writer wins. */
export function setScale(scale: number): void {
  __surface_set_scale(scale);
}
