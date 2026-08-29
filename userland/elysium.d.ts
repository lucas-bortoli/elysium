// Ambient declarations for the Elysium host API: globals (print, timers,
// the JSX factory), the ambient `JSX` namespace, and the `ely:framebuffer`/
// `ely:lifecycle`/`ely:math`/`ely:input`/`ely:image`/`ely:filesystem`/
// `ely:container`/`ely:process` namespaces.

/** A module's own location, expressed as an absolute path rooted at the
 * userland tree.
 * Both are `undefined` for a module that isn't part of the userland tree
 * (an `ely:`-namespaced runtime module). */
interface ImportMeta {
  readonly directoryName: string;
  readonly fileName: string;
}

/** Writes a line to the host's stdout. */
declare function print(...message: any): void;

/** Schedules `callback` to run once, after `delay` milliseconds (clamped to
 * `0` if omitted, negative, or `NaN`), passing any trailing `args` through
 * to it. Returns an id for `clearTimeout`. */
declare function setTimeout(
  callback: (...args: any[]) => void,
  delay?: number,
  ...args: any[]
): number;

/** Cancels a pending timer scheduled by `setTimeout`/`setInterval`/
 * `setImmediate`/`requestAnimationFrame`. A missing or already-fired id is
 * silently ignored. */
declare function clearTimeout(id?: number): void;

/** Like `setTimeout`, but reschedules itself every `delay` milliseconds
 * until cleared with `clearInterval`. */
declare function setInterval(
  callback: (...args: any[]) => void,
  delay?: number,
  ...args: any[]
): number;

/** Cancels a pending timer; see `clearTimeout`. */
declare function clearInterval(id?: number): void;

/** Sugar for `setTimeout(callback, 0, ...args)`. Cancel with `clearTimeout`
 * or `clearImmediate`. */
declare function setImmediate(
  callback: (...args: any[]) => void,
  ...args: any[]
): number;

/** Cancels a pending timer; see `clearTimeout`. */
declare function clearImmediate(id?: number): void;

/** Schedules `callback` to run once, on the next frame, passing the time
 * (in seconds since the VM started) at which that frame began. Returns an
 * id for `cancelAnimationFrame`. */
declare function requestAnimationFrame(
  callback: (timestamp: number) => void,
): number;

/** Cancels a pending timer; see `clearTimeout`. */
declare function cancelAnimationFrame(id?: number): void;

/** Queues `callback` to run once the currently running script/callback
 * finishes, before the next timer or frame. */
declare function queueMicrotask(callback: () => void): void;

declare module "ely:framebuffer" {
  /** The kernel's fixed, curated color palette. Every color a program can
   * draw with is one of these named entries — never a raw, unconstrained
   * RGBA value. */
  export const Color: {
    readonly Red50: 0;
    readonly Red100: 1;
    readonly Red200: 2;
    readonly Red300: 3;
    readonly Red400: 4;
    readonly Red500: 5;
    readonly Red600: 6;
    readonly Red700: 7;
    readonly Red800: 8;
    readonly Red900: 9;
    readonly Red950: 10;
    readonly Orange50: 11;
    readonly Orange100: 12;
    readonly Orange200: 13;
    readonly Orange300: 14;
    readonly Orange400: 15;
    readonly Orange500: 16;
    readonly Orange600: 17;
    readonly Orange700: 18;
    readonly Orange800: 19;
    readonly Orange900: 20;
    readonly Orange950: 21;
    readonly Amber50: 22;
    readonly Amber100: 23;
    readonly Amber200: 24;
    readonly Amber300: 25;
    readonly Amber400: 26;
    readonly Amber500: 27;
    readonly Amber600: 28;
    readonly Amber700: 29;
    readonly Amber800: 30;
    readonly Amber900: 31;
    readonly Amber950: 32;
    readonly Yellow50: 33;
    readonly Yellow100: 34;
    readonly Yellow200: 35;
    readonly Yellow300: 36;
    readonly Yellow400: 37;
    readonly Yellow500: 38;
    readonly Yellow600: 39;
    readonly Yellow700: 40;
    readonly Yellow800: 41;
    readonly Yellow900: 42;
    readonly Yellow950: 43;
    readonly Lime50: 44;
    readonly Lime100: 45;
    readonly Lime200: 46;
    readonly Lime300: 47;
    readonly Lime400: 48;
    readonly Lime500: 49;
    readonly Lime600: 50;
    readonly Lime700: 51;
    readonly Lime800: 52;
    readonly Lime900: 53;
    readonly Lime950: 54;
    readonly Green50: 55;
    readonly Green100: 56;
    readonly Green200: 57;
    readonly Green300: 58;
    readonly Green400: 59;
    readonly Green500: 60;
    readonly Green600: 61;
    readonly Green700: 62;
    readonly Green800: 63;
    readonly Green900: 64;
    readonly Green950: 65;
    readonly Emerald50: 66;
    readonly Emerald100: 67;
    readonly Emerald200: 68;
    readonly Emerald300: 69;
    readonly Emerald400: 70;
    readonly Emerald500: 71;
    readonly Emerald600: 72;
    readonly Emerald700: 73;
    readonly Emerald800: 74;
    readonly Emerald900: 75;
    readonly Emerald950: 76;
    readonly Teal50: 77;
    readonly Teal100: 78;
    readonly Teal200: 79;
    readonly Teal300: 80;
    readonly Teal400: 81;
    readonly Teal500: 82;
    readonly Teal600: 83;
    readonly Teal700: 84;
    readonly Teal800: 85;
    readonly Teal900: 86;
    readonly Teal950: 87;
    readonly Cyan50: 88;
    readonly Cyan100: 89;
    readonly Cyan200: 90;
    readonly Cyan300: 91;
    readonly Cyan400: 92;
    readonly Cyan500: 93;
    readonly Cyan600: 94;
    readonly Cyan700: 95;
    readonly Cyan800: 96;
    readonly Cyan900: 97;
    readonly Cyan950: 98;
    readonly Sky50: 99;
    readonly Sky100: 100;
    readonly Sky200: 101;
    readonly Sky300: 102;
    readonly Sky400: 103;
    readonly Sky500: 104;
    readonly Sky600: 105;
    readonly Sky700: 106;
    readonly Sky800: 107;
    readonly Sky900: 108;
    readonly Sky950: 109;
    readonly Blue50: 110;
    readonly Blue100: 111;
    readonly Blue200: 112;
    readonly Blue300: 113;
    readonly Blue400: 114;
    readonly Blue500: 115;
    readonly Blue600: 116;
    readonly Blue700: 117;
    readonly Blue800: 118;
    readonly Blue900: 119;
    readonly Blue950: 120;
    readonly Indigo50: 121;
    readonly Indigo100: 122;
    readonly Indigo200: 123;
    readonly Indigo300: 124;
    readonly Indigo400: 125;
    readonly Indigo500: 126;
    readonly Indigo600: 127;
    readonly Indigo700: 128;
    readonly Indigo800: 129;
    readonly Indigo900: 130;
    readonly Indigo950: 131;
    readonly Violet50: 132;
    readonly Violet100: 133;
    readonly Violet200: 134;
    readonly Violet300: 135;
    readonly Violet400: 136;
    readonly Violet500: 137;
    readonly Violet600: 138;
    readonly Violet700: 139;
    readonly Violet800: 140;
    readonly Violet900: 141;
    readonly Violet950: 142;
    readonly Purple50: 143;
    readonly Purple100: 144;
    readonly Purple200: 145;
    readonly Purple300: 146;
    readonly Purple400: 147;
    readonly Purple500: 148;
    readonly Purple600: 149;
    readonly Purple700: 150;
    readonly Purple800: 151;
    readonly Purple900: 152;
    readonly Purple950: 153;
    readonly Fuchsia50: 154;
    readonly Fuchsia100: 155;
    readonly Fuchsia200: 156;
    readonly Fuchsia300: 157;
    readonly Fuchsia400: 158;
    readonly Fuchsia500: 159;
    readonly Fuchsia600: 160;
    readonly Fuchsia700: 161;
    readonly Fuchsia800: 162;
    readonly Fuchsia900: 163;
    readonly Fuchsia950: 164;
    readonly Pink50: 165;
    readonly Pink100: 166;
    readonly Pink200: 167;
    readonly Pink300: 168;
    readonly Pink400: 169;
    readonly Pink500: 170;
    readonly Pink600: 171;
    readonly Pink700: 172;
    readonly Pink800: 173;
    readonly Pink900: 174;
    readonly Pink950: 175;
    readonly Rose50: 176;
    readonly Rose100: 177;
    readonly Rose200: 178;
    readonly Rose300: 179;
    readonly Rose400: 180;
    readonly Rose500: 181;
    readonly Rose600: 182;
    readonly Rose700: 183;
    readonly Rose800: 184;
    readonly Rose900: 185;
    readonly Rose950: 186;
    readonly Slate50: 187;
    readonly Slate100: 188;
    readonly Slate200: 189;
    readonly Slate300: 190;
    readonly Slate400: 191;
    readonly Slate500: 192;
    readonly Slate600: 193;
    readonly Slate700: 194;
    readonly Slate800: 195;
    readonly Slate900: 196;
    readonly Slate950: 197;
    readonly Gray50: 198;
    readonly Gray100: 199;
    readonly Gray200: 200;
    readonly Gray300: 201;
    readonly Gray400: 202;
    readonly Gray500: 203;
    readonly Gray600: 204;
    readonly Gray700: 205;
    readonly Gray800: 206;
    readonly Gray900: 207;
    readonly Gray950: 208;
    readonly Zinc50: 209;
    readonly Zinc100: 210;
    readonly Zinc200: 211;
    readonly Zinc300: 212;
    readonly Zinc400: 213;
    readonly Zinc500: 214;
    readonly Zinc600: 215;
    readonly Zinc700: 216;
    readonly Zinc800: 217;
    readonly Zinc900: 218;
    readonly Zinc950: 219;
    readonly Neutral50: 220;
    readonly Neutral100: 221;
    readonly Neutral200: 222;
    readonly Neutral300: 223;
    readonly Neutral400: 224;
    readonly Neutral500: 225;
    readonly Neutral600: 226;
    readonly Neutral700: 227;
    readonly Neutral800: 228;
    readonly Neutral900: 229;
    readonly Neutral950: 230;
    readonly Stone50: 231;
    readonly Stone100: 232;
    readonly Stone200: 233;
    readonly Stone300: 234;
    readonly Stone400: 235;
    readonly Stone500: 236;
    readonly Stone600: 237;
    readonly Stone700: 238;
    readonly Stone800: 239;
    readonly Stone900: 240;
    readonly Stone950: 241;
    readonly Taupe50: 242;
    readonly Taupe100: 243;
    readonly Taupe200: 244;
    readonly Taupe300: 245;
    readonly Taupe400: 246;
    readonly Taupe500: 247;
    readonly Taupe600: 248;
    readonly Taupe700: 249;
    readonly Taupe800: 250;
    readonly Taupe900: 251;
    readonly Taupe950: 252;
    readonly Mauve50: 253;
    readonly Mauve100: 254;
    readonly Mauve200: 255;
    readonly Mauve300: 256;
    readonly Mauve400: 257;
    readonly Mauve500: 258;
    readonly Mauve600: 259;
    readonly Mauve700: 260;
    readonly Mauve800: 261;
    readonly Mauve900: 262;
    readonly Mauve950: 263;
    readonly Mist50: 264;
    readonly Mist100: 265;
    readonly Mist200: 266;
    readonly Mist300: 267;
    readonly Mist400: 268;
    readonly Mist500: 269;
    readonly Mist600: 270;
    readonly Mist700: 271;
    readonly Mist800: 272;
    readonly Mist900: 273;
    readonly Mist950: 274;
    readonly Olive50: 275;
    readonly Olive100: 276;
    readonly Olive200: 277;
    readonly Olive300: 278;
    readonly Olive400: 279;
    readonly Olive500: 280;
    readonly Olive600: 281;
    readonly Olive700: 282;
    readonly Olive800: 283;
    readonly Olive900: 284;
    readonly Olive950: 285;
    readonly Black: 286;
    readonly White: 287;
  };

  /** A color from the kernel's fixed palette, as one of `Color`'s named
   * entries (e.g. `Color.Slate900`). */
  export type Color = (typeof Color)[keyof typeof Color];

  /** The kernel's set of built-in bitmap fonts. */
  export const Font: {
    readonly Cozette: 0;
  };

  /** One of `Font`'s named entries (e.g. `Font.Cozette`). */
  export type Font = (typeof Font)[keyof typeof Font];

  /** Thrown by any drawing call made from outside a currently running draw
   * handler (see `addDrawHandler`). */
  export class DrawOutsideHandlerError extends Error {
    constructor();
  }

  /** Thrown by `popTransform`/`popClip` when nothing is left to pop. */
  export class UnbalancedStackError extends Error {
    constructor(what: string);
  }

  export type DrawTickerId = number;

  /** The framebuffer's logical width, in pixels. */
  export function getWidth(): number;

  /** The framebuffer's logical height, in pixels. */
  export function getHeight(): number;

  /** The framebuffer's logical size, in pixels. */
  export function getSize2d(): import("ely:math").Size2d;

  /** Registers `handler` to run once per frame; `clearScreen`/`fillRectangle`
   * only take effect when called from inside a running handler. Returns an
   * id for `removeDrawHandler`. */
  export function addDrawHandler(handler: () => void): DrawTickerId;

  /** Stops calling the draw handler registered under `id`. */
  export function removeDrawHandler(id: DrawTickerId): void;

  /** Clears the whole screen to `color`. */
  export function clearScreen(color: Color): void;

  /** Fills an axis-aligned rectangle at `(x, y)`, `w` wide and `h` tall, with `color`. */
  export function fillRectangle(
    x: number,
    y: number,
    w: number,
    h: number,
    color: Color,
  ): void;

  /** Which part of an image to draw, and how to place it. */
  export interface DrawImageOptions {
    /** The left edge of the part of the image to draw. Defaults to 0. */
    sx?: number;
    /** The top edge of the part of the image to draw. Defaults to 0. */
    sy?: number;
    /** The width of the part to draw. Defaults to the rest of the image,
     * to the right of `sx`. */
    sw?: number;
    /** The height of the part to draw. Defaults to the rest of the image,
     * below `sy`. */
    sh?: number;
    /** Draws the image this many times its natural size. A single number
     * scales both axes alike. Whole numbers keep it pixel-crisp; anything
     * else lands its pixels unevenly, since nothing is smoothed. */
    scale?: number | import("ely:math").Vector2d;
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

  /** Draws `image` with its top-left corner at `(x, y)`. With no options it
   * goes on at its natural size, whole; options take part of it instead,
   * resize it, or mirror it. */
  export function drawImage(
    image: import("ely:image").Image | import("ely:image").ImageId,
    x: number,
    y: number,
    options?: DrawImageOptions,
  ): void;

  /** Draws `image` at `(x, y)`, turned `radians` about the point
   * `originX`, `originY` within it — clockwise on screen, since `y` grows
   * downward. The origin defaults to the image's top-left corner. */
  export function drawImageRotated(
    image: import("ely:image").Image | import("ely:image").ImageId,
    x: number,
    y: number,
    radians: number,
    options?: DrawImageRotatedOptions,
  ): void;

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

  /** Draws `text` in `color` with its top-left corner at `(x, y)`, using
   * one of the kernel's built-in bitmap fonts.
   *
   * Passing options instead of a bare font aligns the text against `x`
   * rather than starting from it, wraps it to a width, or draws it at a
   * whole-number multiple of the font's size. Line breaks in `text` are
   * honoured either way. Only takes effect from inside a running draw
   * handler. */
  export function drawText(
    x: number,
    y: number,
    text: string,
    color: Color,
    fontOrOptions?: Font | TextOptions,
  ): void;

  /** The pixel box `text` would occupy if drawn with the same options —
   * the width of its widest line and the height of the whole block. A
   * query, not a draw call: usable from anywhere to lay text out without
   * assuming the font's size. */
  export function measureText(
    text: string,
    fontOrOptions?: Font | TextOptions,
  ): import("ely:math").Size2d;

  /** Sets how many physical pixels the window draws each logical pixel as
   * — an integer of at least 1. Takes effect on the next frame; unlike
   * `clearScreen`/`fillRectangle`, can be called from anywhere, not just
   * from inside a draw handler. There is one window shared by every
   * process, so this is a global, last-writer-wins setting. */
  export function setScale(scale: number): void;

  /** The palette entry closest to the RGB triplet `(r, g, b)` (each `0-255`). */
  export function nearestColor(r: number, g: number, b: number): Color;

  /** How a path decides which of its regions count as inside, where its
   * outline crosses over itself. `"nonzero"` counts a region inside when
   * the outline winds around it at all; `"evenodd"` alternates, so a shape
   * drawn inside another punches a hole in it. */
  export type FillRule = "nonzero" | "evenodd";

  /** How a stroke finishes at the two loose ends of an open path. */
  export type LineCap = "butt" | "round" | "square";

  /** How a stroke turns a corner where two segments meet. */
  export type LineJoin = "miter" | "round" | "bevel";

  /** Starts a new path, discarding whatever was being described before it.
   * There is one path under construction at a time, and the shape calls
   * that describe a whole path of their own start a new one. */
  export function beginPath(): void;

  /** Starts a new contour of the current path at `(x, y)`, without drawing
   * anything on the way there. */
  export function moveTo(x: number, y: number): void;

  /** Extends the current path with a straight segment to `(x, y)`. */
  export function lineTo(x: number, y: number): void;

  /** Extends the current path with a curve to `(x, y)` that bends toward
   * the single control point `(cx, cy)` without passing through it. */
  export function quadraticTo(
    cx: number,
    cy: number,
    x: number,
    y: number,
  ): void;

  /** Extends the current path with a curve to `(x, y)` that leaves along
   * `(c1x, c1y)` and arrives along `(c2x, c2y)`. */
  export function cubicTo(
    c1x: number,
    c1y: number,
    c2x: number,
    c2y: number,
    x: number,
    y: number,
  ): void;

  /** Closes the current contour with a straight segment back to where it
   * started. */
  export function closePath(): void;

  /** Fills the inside of the current path with `color`, leaving the path in
   * place so it can be stroked afterwards. */
  export function fillPath(color: Color, rule?: FillRule): void;

  /** Draws a line of `thickness` along the current path in `color`,
   * straddling the path with half its thickness to either side. */
  export function strokePath(
    color: Color,
    thickness?: number,
    cap?: LineCap,
    join?: LineJoin,
  ): void;

  /** How `pushTransform` should move the coordinate space. Applied in the
   * order written: a shape is scaled, then rotated, then shifted. */
  export interface Transform {
    /** Shifts by this much, in the coordinates outside the transform. */
    translate?: import("ely:math").Vector2d;
    /** Scales about the origin. A single number scales both axes alike. */
    scale?: import("ely:math").Vector2d | number;
    /** Turns about the origin, in radians — clockwise on screen, since `y`
     * grows downward. */
    rotate?: number;
  }

  /** Moves the coordinate space everything drawn afterwards is placed in,
   * until the matching `popTransform`. Transforms nest: pushing a second
   * one applies inside the first rather than replacing it. */
  export function pushTransform(transform: Transform): void;

  /** Restores the coordinate space in effect before the matching
   * `pushTransform`. */
  export function popTransform(): void;

  /** Confines everything drawn afterwards to the rectangle at `(x, y)`,
   * until the matching `popClip`. Clips nest by narrowing. Starts a new
   * path. */
  export function pushClip(x: number, y: number, w: number, h: number): void;

  /** Confines everything drawn afterwards to the inside of the current
   * path, until the matching `popClip`. */
  export function pushClipPath(rule?: FillRule): void;

  /** Restores the region in effect before the matching `pushClip`. */
  export function popClip(): void;

  /** Draws the outline of an axis-aligned rectangle at `(x, y)`. The
   * outline straddles the rectangle's edge, so it doesn't cover exactly the
   * pixels `fillRectangle` would. */
  export function strokeRectangle(
    x: number,
    y: number,
    w: number,
    h: number,
    color: Color,
    thickness?: number,
  ): void;

  /** Fills a rectangle at `(x, y)` whose corners are rounded off by
   * `radius`, clamped to half the shorter side. */
  export function fillRoundedRectangle(
    x: number,
    y: number,
    w: number,
    h: number,
    radius: number,
    color: Color,
  ): void;

  /** Draws the outline of a rectangle at `(x, y)` with corners rounded off
   * by `radius`. */
  export function strokeRoundedRectangle(
    x: number,
    y: number,
    w: number,
    h: number,
    radius: number,
    color: Color,
    thickness?: number,
  ): void;

  /** Draws a straight line from `(x1, y1)` to `(x2, y2)`. A line straddles
   * the coordinates it runs along, so run it down the middle of a pixel
   * column — `x + 0.5` — for one crisp line. */
  export function drawLine(
    x1: number,
    y1: number,
    x2: number,
    y2: number,
    color: Color,
    thickness?: number,
  ): void;

  /** Draws straight lines through `points` in order, leaving the two ends
   * loose. Fewer than two points draws nothing. */
  export function drawPolyline(
    points: readonly import("ely:math").Vector2d[],
    color: Color,
    thickness?: number,
  ): void;

  /** Fills a circle of radius `r` centred on `(cx, cy)`. */
  export function fillCircle(
    cx: number,
    cy: number,
    r: number,
    color: Color,
  ): void;

  /** Draws the outline of a circle of radius `r` centred on `(cx, cy)`. */
  export function strokeCircle(
    cx: number,
    cy: number,
    r: number,
    color: Color,
    thickness?: number,
  ): void;

  /** Fills an axis-aligned ellipse centred on `(cx, cy)`, reaching `rx` to
   * either side and `ry` above and below. */
  export function fillEllipse(
    cx: number,
    cy: number,
    rx: number,
    ry: number,
    color: Color,
  ): void;

  /** Draws the outline of an axis-aligned ellipse centred on `(cx, cy)`. */
  export function strokeEllipse(
    cx: number,
    cy: number,
    rx: number,
    ry: number,
    color: Color,
    thickness?: number,
  ): void;

  /** Draws the piece of a circle's rim running from `startRad` to `endRad`,
   * in radians measured from `+x` and increasing clockwise on screen. The
   * sweep follows which way round the two angles are named. */
  export function drawArc(
    cx: number,
    cy: number,
    r: number,
    startRad: number,
    endRad: number,
    color: Color,
    thickness?: number,
  ): void;

  /** Fills the triangle with corners `a`, `b` and `c`. */
  export function fillTriangle(
    a: import("ely:math").Vector2d,
    b: import("ely:math").Vector2d,
    c: import("ely:math").Vector2d,
    color: Color,
  ): void;

  /** Fills the shape enclosed by `points`, joined in order and closed back
   * to the first. Fewer than three points draws nothing. `rule` decides
   * what counts as inside where the outline crosses itself. */
  export function fillPolygon(
    points: readonly import("ely:math").Vector2d[],
    color: Color,
    rule?: FillRule,
  ): void;

  /** Draws the outline of the shape enclosed by `points`, closed back to
   * the first — unlike `drawPolyline`, which leaves its ends loose. */
  export function strokePolygon(
    points: readonly import("ely:math").Vector2d[],
    color: Color,
    thickness?: number,
  ): void;

  /** Sets the single pixel that `(x, y)` falls inside. Coordinates name the
   * corners of the pixel grid, so `(3, 4)` and `(3.5, 4.5)` both set the
   * same pixel. */
  export function setPixel(x: number, y: number, color: Color): void;

  /** Sets every pixel in `points` to the same color. */
  export function drawPixels(
    points: readonly import("ely:math").Vector2d[],
    color: Color,
  ): void;
}

declare module "ely:math" {
  /** A 2D point or offset. */
  export interface Vector2d {
    x: number;
    y: number;
  }

  /** A 2D size. */
  export interface Size2d {
    width: number;
    height: number;
  }

  /** An axis-aligned rectangle. */
  export interface Rectangle {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  /** Tolerance `floatEquals`/`vector2Equals` compare within, scaled by the
   * magnitude of the values being compared — coarser than `Number.EPSILON`,
   * whose ~2.22e-16 gap near `1.0` is too tight once error has accumulated
   * over a few float operations. */
  export const EPSILON = 0.000001;

  /** Whether `a` and `b` are equal within `EPSILON`, scaled by their magnitude. */
  export function floatEquals(a: number, b: number): boolean;

  /** The vector `{ x: 0, y: 0 }`. */
  export function vector2Zero(): Vector2d;

  /** The vector `{ x: 1, y: 1 }`. */
  export function vector2One(): Vector2d;

  /** Whether `a` and `b` are equal within `EPSILON`, component-wise. */
  export function vector2Equals(a: Vector2d, b: Vector2d): boolean;

  /** Adds `a` and `b` component-wise. */
  export function vector2Add(a: Vector2d, b: Vector2d): Vector2d;

  /** Adds `value` to both of `v`'s components. */
  export function vector2AddValue(v: Vector2d, value: number): Vector2d;

  /** Subtracts `b` from `a` component-wise. */
  export function vector2Subtract(a: Vector2d, b: Vector2d): Vector2d;

  /** Subtracts `value` from both of `v`'s components. */
  export function vector2SubtractValue(v: Vector2d, value: number): Vector2d;

  /** Scales `v` by `factor`. */
  export function vector2Scale(v: Vector2d, factor: number): Vector2d;

  /** The straight-line distance between `a` and `b`. */
  export function vector2Distance(a: Vector2d, b: Vector2d): number;

  /** The point `t` of the way from `a` to `b` (`t = 0` is `a`, `t = 1` is `b`). */
  export function vector2Lerp(a: Vector2d, b: Vector2d, t: number): Vector2d;

  /** `v` with both components negated. */
  export function vector2Negate(v: Vector2d): Vector2d;

  /** The length of `v`. */
  export function vector2Length(v: Vector2d): number;

  /** The squared length of `v`, cheaper than `vector2Length` when only comparing magnitudes. */
  export function vector2LengthSquared(v: Vector2d): number;

  /** The dot product of `a` and `b`. */
  export function vector2Dot(a: Vector2d, b: Vector2d): number;

  /** `v` scaled to length 1, or `vector2Zero()` if `v` has zero length. */
  export function vector2Normalize(v: Vector2d): Vector2d;

  /** `v` rotated by `radians` around the origin. */
  export function vector2Rotate(v: Vector2d, radians: number): Vector2d;

  /** `v` with each component restricted to the `[min, max]` range. */
  export function vector2Clamp(
    v: Vector2d,
    min: Vector2d,
    max: Vector2d,
  ): Vector2d;

  /** The angle of `v`, in radians, measured counterclockwise from the positive x-axis. */
  export function vector2Angle(v: Vector2d): number;

  /** Whether `a` and `b` overlap by any amount. */
  export function rectangleIntersects(a: Rectangle, b: Rectangle): boolean;

  /** Whether `point` falls inside `rect`. */
  export function rectangleContains(rect: Rectangle, point: Vector2d): boolean;

  /** `value`, restricted to the `[min, max]` range. */
  export function clamp(value: number, min: number, max: number): number;

  /** The value `t` of the way from `a` to `b` (`t = 0` is `a`, `t = 1` is `b`). */
  export function lerp(a: number, b: number, t: number): number;

  /** `degrees` converted to radians. */
  export function degToRad(degrees: number): number;

  /** `radians` converted to degrees. */
  export function radToDeg(radians: number): number;
}

declare module "ely:lifecycle" {
  /** Registers `handler` to run once, right after the program's top-level
   * code finishes evaluating, once timers, tickers, and draw handlers are
   * live. */
  export function addPostInitHandler(handler: () => void): void;

  /** Resolves after `ms` milliseconds — `setTimeout` as an awaitable.
   * @warn Awaiting this from module top level deadlocks. See
   * Documentation/Multitasking.md. */
  export function delay(ms: number): Promise<void>;

  export type TickerId = number;

  /** Registers `handler` to run once per frame with the time (in seconds)
   * since the previous frame. Returns an id for `removeUpdateTicker`. */
  export function addUpdateTicker(handler: (dt: number) => void): TickerId;

  /** Stops calling the ticker registered under `id`. */
  export function removeUpdateTicker(id: TickerId): void;

  /** The time, in seconds, since the previous frame. */
  export function getDeltaTime(): number;
}

declare module "ely:input" {
  /** The pointer's current x position. */
  export function getPointerX(): number;

  /** The pointer's current y position. */
  export function getPointerY(): number;

  /** The pointer's current position. */
  export function getPointerPosition(): import("ely:math").Vector2d;

  /** Whether the pointer's button is currently held down. */
  export function isPointerDown(): boolean;

  /** Whether the pointer's button is currently not held down. */
  export function isPointerUp(): boolean;

  /** Whether the pointer's button was pressed this frame. */
  export function wasPointerPressed(): boolean;

  /** Whether the pointer's button was released this frame. */
  export function wasPointerReleased(): boolean;

  /** How far the pointer moved since last frame. */
  export function getPointerDelta(): import("ely:math").Vector2d;

  /** How far the scroll wheel moved since last frame. */
  export function getScrollDelta(): number;

  /** Every physical key the keyboard device recognizes, identified by its
   * position on the keyboard rather than the character it produces — e.g.
   * `KeyW` is the key in the "W" position on a US layout, whatever an
   * AZERTY keyboard prints on it. */
  export const Key: {
    readonly Backquote: 0;
    readonly Backslash: 1;
    readonly BracketLeft: 2;
    readonly BracketRight: 3;
    readonly Comma: 4;
    readonly Digit0: 5;
    readonly Digit1: 6;
    readonly Digit2: 7;
    readonly Digit3: 8;
    readonly Digit4: 9;
    readonly Digit5: 10;
    readonly Digit6: 11;
    readonly Digit7: 12;
    readonly Digit8: 13;
    readonly Digit9: 14;
    readonly Equal: 15;
    readonly IntlBackslash: 16;
    readonly IntlRo: 17;
    readonly IntlYen: 18;
    readonly KeyA: 19;
    readonly KeyB: 20;
    readonly KeyC: 21;
    readonly KeyD: 22;
    readonly KeyE: 23;
    readonly KeyF: 24;
    readonly KeyG: 25;
    readonly KeyH: 26;
    readonly KeyI: 27;
    readonly KeyJ: 28;
    readonly KeyK: 29;
    readonly KeyL: 30;
    readonly KeyM: 31;
    readonly KeyN: 32;
    readonly KeyO: 33;
    readonly KeyP: 34;
    readonly KeyQ: 35;
    readonly KeyR: 36;
    readonly KeyS: 37;
    readonly KeyT: 38;
    readonly KeyU: 39;
    readonly KeyV: 40;
    readonly KeyW: 41;
    readonly KeyX: 42;
    readonly KeyY: 43;
    readonly KeyZ: 44;
    readonly Minus: 45;
    readonly Period: 46;
    readonly Quote: 47;
    readonly Semicolon: 48;
    readonly Slash: 49;
    readonly AltLeft: 50;
    readonly AltRight: 51;
    readonly Backspace: 52;
    readonly CapsLock: 53;
    readonly ContextMenu: 54;
    readonly ControlLeft: 55;
    readonly ControlRight: 56;
    readonly Enter: 57;
    readonly SuperLeft: 58;
    readonly SuperRight: 59;
    readonly ShiftLeft: 60;
    readonly ShiftRight: 61;
    readonly Space: 62;
    readonly Tab: 63;
    readonly Convert: 64;
    readonly KanaMode: 65;
    readonly Lang1: 66;
    readonly Lang2: 67;
    readonly Lang3: 68;
    readonly Lang4: 69;
    readonly Lang5: 70;
    readonly NonConvert: 71;
    readonly Delete: 72;
    readonly End: 73;
    readonly Help: 74;
    readonly Home: 75;
    readonly Insert: 76;
    readonly PageDown: 77;
    readonly PageUp: 78;
    readonly ArrowDown: 79;
    readonly ArrowLeft: 80;
    readonly ArrowRight: 81;
    readonly ArrowUp: 82;
    readonly NumLock: 83;
    readonly Numpad0: 84;
    readonly Numpad1: 85;
    readonly Numpad2: 86;
    readonly Numpad3: 87;
    readonly Numpad4: 88;
    readonly Numpad5: 89;
    readonly Numpad6: 90;
    readonly Numpad7: 91;
    readonly Numpad8: 92;
    readonly Numpad9: 93;
    readonly NumpadAdd: 94;
    readonly NumpadBackspace: 95;
    readonly NumpadClear: 96;
    readonly NumpadClearEntry: 97;
    readonly NumpadComma: 98;
    readonly NumpadDecimal: 99;
    readonly NumpadDivide: 100;
    readonly NumpadEnter: 101;
    readonly NumpadEqual: 102;
    readonly NumpadHash: 103;
    readonly NumpadMemoryAdd: 104;
    readonly NumpadMemoryClear: 105;
    readonly NumpadMemoryRecall: 106;
    readonly NumpadMemoryStore: 107;
    readonly NumpadMemorySubtract: 108;
    readonly NumpadMultiply: 109;
    readonly NumpadParenLeft: 110;
    readonly NumpadParenRight: 111;
    readonly NumpadStar: 112;
    readonly NumpadSubtract: 113;
    readonly Escape: 114;
    readonly Fn: 115;
    readonly FnLock: 116;
    readonly PrintScreen: 117;
    readonly ScrollLock: 118;
    readonly Pause: 119;
    readonly BrowserBack: 120;
    readonly BrowserFavorites: 121;
    readonly BrowserForward: 122;
    readonly BrowserHome: 123;
    readonly BrowserRefresh: 124;
    readonly BrowserSearch: 125;
    readonly BrowserStop: 126;
    readonly Eject: 127;
    readonly LaunchApp1: 128;
    readonly LaunchApp2: 129;
    readonly LaunchMail: 130;
    readonly MediaPlayPause: 131;
    readonly MediaSelect: 132;
    readonly MediaStop: 133;
    readonly MediaTrackNext: 134;
    readonly MediaTrackPrevious: 135;
    readonly Power: 136;
    readonly Sleep: 137;
    readonly AudioVolumeDown: 138;
    readonly AudioVolumeMute: 139;
    readonly AudioVolumeUp: 140;
    readonly WakeUp: 141;
    readonly Meta: 142;
    readonly Hyper: 143;
    readonly Turbo: 144;
    readonly Abort: 145;
    readonly Resume: 146;
    readonly Suspend: 147;
    readonly Again: 148;
    readonly Copy: 149;
    readonly Cut: 150;
    readonly Find: 151;
    readonly Open: 152;
    readonly Paste: 153;
    readonly Props: 154;
    readonly Select: 155;
    readonly Undo: 156;
    readonly Hiragana: 157;
    readonly Katakana: 158;
    readonly F1: 159;
    readonly F2: 160;
    readonly F3: 161;
    readonly F4: 162;
    readonly F5: 163;
    readonly F6: 164;
    readonly F7: 165;
    readonly F8: 166;
    readonly F9: 167;
    readonly F10: 168;
    readonly F11: 169;
    readonly F12: 170;
    readonly F13: 171;
    readonly F14: 172;
    readonly F15: 173;
    readonly F16: 174;
    readonly F17: 175;
    readonly F18: 176;
    readonly F19: 177;
    readonly F20: 178;
    readonly F21: 179;
    readonly F22: 180;
    readonly F23: 181;
    readonly F24: 182;
    readonly F25: 183;
    readonly F26: 184;
    readonly F27: 185;
    readonly F28: 186;
    readonly F29: 187;
    readonly F30: 188;
    readonly F31: 189;
    readonly F32: 190;
    readonly F33: 191;
    readonly F34: 192;
    readonly F35: 193;
  };

  /** A key from the keyboard device, as one of `Key`'s named entries (e.g.
   * `Key.KeyW`). */
  export type Key = (typeof Key)[keyof typeof Key];

  /** Whether `key` is currently held down. */
  export function isKeyDown(key: Key): boolean;

  /** Whether `key` is currently not held down. */
  export function isKeyUp(key: Key): boolean;

  /** Whether `key` was pressed this frame. */
  export function wasKeyPressed(key: Key): boolean;

  /** Whether `key` was released this frame. */
  export function wasKeyReleased(key: Key): boolean;
}

declare module "ely:filesystem" {
  /** Thrown when a userland filesystem path is given relative instead of
   * absolute. Every such path must start with `/` and is resolved against
   * the userland root, never wherever the calling module happens to live —
   * `import.meta.directoryName`/`fileName` are how a module finds its own
   * location to build an absolute path from. */
  export class RelativePathError extends Error {
    constructor(message: string);
  }

  export const sep: string;

  /** Resolves `.`/`..` segments and collapses redundant slashes. The result
   * is always rooted at `/`; a `..` that would go above the root is dropped
   * rather than escaping it. */
  export function normalize(path: string): string;

  /** Joins path segments into one and normalizes the result. */
  export function join(...paths: string[]): string;

  /** The final segment of a path, with `ext` stripped from the end if
   * present. Trailing slashes are ignored. */
  export function extractBaseName(path: string, ext?: string): string;

  /** The path up to, but not including, the final segment. Trailing
   * slashes are ignored, and the root's own parent is itself. */
  export function extractDirectoryName(path: string): string;

  /** The final segment's extension, including the leading `.`, or `""` if
   * it has none. A leading dot, as in `.bashrc`, is not itself an
   * extension. */
  export function extractExtension(path: string): string;

  /** Builds an absolute path by resolving segments right-to-left, stopping
   * at the first one that's already absolute. Equivalent to `join` when
   * none of the segments are absolute. */
  export function resolve(...paths: string[]): string;

  /** Replaces characters outside `[a-zA-Z0-9_.-]` with `_` and truncates to
   * `maxLength`. `.` and `..` are replaced outright, since they would
   * otherwise pass through unchanged and remain usable for directory
   * traversal. */
  export function sanitizeName(filename: string, maxLength?: number): string;

  // The classes below are a shared vocabulary reused across every
  // function — mirroring `std::io::ErrorKind` being one shared set of
  // variants for every Rust filesystem call, rather than a bespoke error
  // type per call site. Each function's exported error type
  // (`ReadFileError`, `WriteFileError`, ...) is just the union of
  // whichever of these it can actually throw.

  /** A path, or a component of it, doesn't exist. */
  export class NotFoundError extends Error {
    constructor(message: string);
  }

  /** A file operation (read, write, or removing a file) found a directory
   * instead. */
  export class IsADirectoryError extends Error {
    constructor(message: string);
  }

  /** A directory operation (`createDirectory`, `listDirectory`) is
   * blocked by an existing non-directory somewhere along the path. */
  export class NotADirectoryError extends Error {
    constructor(message: string);
  }

  /** `readTextFile`'s bytes aren't valid UTF-8. */
  export class TextDecodeError extends Error {
    constructor(message: string);
  }

  /** Anything else the underlying operation failed with — permission
   * denied, disk full, and so on. `.message` carries the full underlying
   * OS error text plus the operation and path involved, so it stays
   * useful for debugging even though it isn't one of the
   * specifically-typed cases above. */
  export class UnknownError extends Error {
    constructor(message: string);
  }

  /** Every case `readFile` can actually throw natively (beyond
   * `RelativePathError`, thrown before any native call). */
  export type ReadFileError = NotFoundError | IsADirectoryError | UnknownError;

  /** Every case `readTextFile` can actually throw natively — like
   * `ReadFileError`, plus `TextDecodeError` for non-UTF-8 bytes. */
  export type ReadTextFileError =
    | NotFoundError
    | IsADirectoryError
    | TextDecodeError
    | UnknownError;

  /** Every case `writeFile` can actually throw natively. */
  export type WriteFileError = NotFoundError | IsADirectoryError | UnknownError;

  /** Every case `writeTextFile` can actually throw natively. */
  export type WriteTextFileError =
    | NotFoundError
    | IsADirectoryError
    | UnknownError;

  /** Every case `remove` can actually throw natively. */
  export type DeleteError = NotFoundError | UnknownError;

  /** Every case `createDirectory` can actually throw natively. */
  export type CreateDirectoryError = NotADirectoryError | UnknownError;

  /** Every case `listDirectory` can actually throw natively. */
  export type ListDirectoryError =
    | NotFoundError
    | NotADirectoryError
    | UnknownError;

  /** Every case `stat` can actually throw natively. */
  export type StatError = NotFoundError | UnknownError;

  /** A byte range within a file, used by `readFile`/`writeFile` to operate
   * on part of a file instead of the whole thing. Both fields are
   * optional: `offset` defaults to `0`, `length` defaults to "the rest of
   * the file" for `readFile` and "all of `data`" for `writeFile`. */
  export interface ByteRange {
    offset?: number;
    length?: number;
  }

  /** The result of `stat`, and of each entry `listDirectory` returns.
   * `path` is the entry's own absolute, virtual path — for
   * `listDirectory`, that's the listed directory joined with the entry's
   * name, not just the name by itself. */
  export type EntryStat =
    | { readonly kind: "File"; readonly size: number; readonly path: string }
    | { readonly kind: "Directory"; readonly path: string };

  /** Reads the whole file at `path`, or — if `range` is given — only the
   * bytes from `range.offset` (default `0`) spanning at most
   * `range.length` bytes (default: to the end of the file). `path` must
   * be absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotFoundError} if `path` doesn't exist.
   * @throws {IsADirectoryError} if `path` names a directory.
   * @throws {UnknownError} on any other failure. */
  export function readFile(
    path: string,
    range?: import("ely:container").Option<ByteRange>,
  ): Uint8Array;

  /** Writes `data` to `path`. With no `range`, the file is truncated and
   * replaced entirely. With `range`, the file is patched in place:
   * writing starts at `range.offset` (default `0`), and bytes outside the
   * written span are left untouched. `path` must be absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotFoundError} if `path`'s parent directory doesn't exist.
   * @throws {IsADirectoryError} if `path` names a directory.
   * @throws {UnknownError} on any other failure. */
  export function writeFile(
    path: string,
    data: Uint8Array,
    range?: import("ely:container").Option<ByteRange>,
  ): void;

  /** Reads the file at `path` as UTF-8 text. `path` must be absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotFoundError} if `path` doesn't exist.
   * @throws {IsADirectoryError} if `path` names a directory.
   * @throws {TextDecodeError} if `path`'s bytes aren't valid UTF-8.
   * @throws {UnknownError} on any other failure. */
  export function readTextFile(path: string): string;

  /** Writes `text` to `path`, truncating and replacing it entirely. `path`
   * must be absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotFoundError} if `path`'s parent directory doesn't exist.
   * @throws {IsADirectoryError} if `path` names a directory.
   * @throws {UnknownError} on any other failure. */
  export function writeTextFile(path: string, text: string): void;

  /** Removes the file or directory at `path`. A directory is removed
   * recursively, along with everything inside it. `path` must be
   * absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotFoundError} if `path` doesn't exist.
   * @throws {UnknownError} on any other failure. */
  export function remove(path: string): void;

  /** Creates the directory at `path`, along with any missing parent
   * directories. `path` must be absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotADirectoryError} if a non-directory already exists
   * somewhere along `path`.
   * @throws {UnknownError} on any other failure. */
  export function createDirectory(path: string): void;

  /** Lists the entries directly inside the directory at `path`, each as
   * an `EntryStat` whose `path` is that entry's own absolute path (the
   * listed directory joined with its name). `path` must be absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotFoundError} if `path` doesn't exist.
   * @throws {NotADirectoryError} if `path` names a file.
   * @throws {UnknownError} on any other failure. */
  export function listDirectory(path: string): EntryStat[];

  /** Reports whether `path` is a file or a directory, its size in bytes
   * if it's a file, and its own absolute path. `path` must be absolute.
   * @throws {RelativePathError} if `path` isn't absolute.
   * @throws {NotFoundError} if `path` doesn't exist.
   * @throws {UnknownError} on any other failure. */
  export function stat(path: string): EntryStat;
}

declare module "ely:container" {
  /** A value that may be absent: either a `T`, or `undefined`/`null`
   * standing in for "nothing". Mirrors Rust's `Option<T>`, adapted to
   * accept either of JavaScript's two nullish values rather than defining
   * a single new one. */
  export type Option<T> = T | undefined | null;

  /** A total predicate: any value is a valid `Option<T>` (either `T`
   * itself or absent). Exists to narrow an `unknown` into `Option<T>` in
   * generic code; pairs with `hasValue` for the complementary "is it
   * actually there" check. */
  export function isOption<T>(arg: unknown): arg is Option<T>;

  /** Narrows `Option<T>` down to a present `T`. */
  export function hasValue<T>(arg: Option<T>): arg is T;

  /** The canonical absent `Option<T>`, always `undefined`. */
  export function none<T>(): Option<T>;

  /** Wraps a definite value as a present `Option<T>`. */
  export function some<T>(value: T): Option<T>;

  /** Returns `arg` if present, else `fallback`. */
  export function getOrElse<T>(arg: Option<T>, fallback: T): T;

  /** Transforms the contained value if present; passes an absence through
   * unchanged. */
  export function map<T, U>(arg: Option<T>, fn: (value: T) => U): Option<U>;

  /** Returns the contained value, or throws `OptionUnwrapError` if
   * absent.
   * @throws {OptionUnwrapError} if `arg` is absent. */
  export function unwrap<T>(arg: Option<T>): T;

  /** Thrown by `unwrap` when the `Option<T>` it's given is absent. */
  export class OptionUnwrapError extends Error {
    constructor(message: string);
  }
}

declare module "ely:image" {
  /** Opaque handle to a loaded image, returned by `loadImage`. */
  export type ImageId = number;

  /** A loaded, palette-quantized picture, ready to be drawn with
   * `ely:framebuffer`'s `drawImage`. */
  export interface Image {
    readonly id: ImageId;
    readonly width: number;
    readonly height: number;
  }

  /** Thrown by `loadImage` when `path` doesn't exist, escapes the
   * userland root, or isn't a decodable PNG. */
  export class ImageLoadError extends Error {
    constructor(message: string);
  }

  /** Loads the PNG at `path`, an absolute path resolved against the
   * userland root. Throws `RelativePathError` (from `ely:filesystem`) if
   * `path` doesn't start with `/`. */
  export function loadImage(path: string): Image;

  /** Frees a loaded image early. An image not explicitly unloaded stays
   * loaded for the lifetime of the program, not until nothing references
   * it anymore — dropping every reference to an `Image` does not free it. */
  export function unloadImage(image: Image | ImageId): void;
}

declare module "ely:process" {
  import type { Option } from "ely:container";

  /** A process's identity in the kernel's table. Just a number — there is
   * no wrapper object. `0` is the kernel. */
  export type ProcessHandle = number;

  /** A message as it arrives at `onMessage`: the sender's `kind` and
   * payload, plus who it came `from` and who it was addressed `to`. A
   * `kind` starting with `ely:` was sent by the kernel itself (today only
   * `"ely:exit"`). */
  export interface Envelope {
    kind: string;
    from: ProcessHandle;
    to: ProcessHandle;
    data: Option<unknown>;
  }

  /** What a caller passes to `postMessage`: a `kind` label and an optional
   * payload (anything `JSON.stringify` accepts; `Option`'s absent value
   * arrives as `null`). */
  export interface Message {
    kind: string;
    data: Option<unknown>;
  }

  /** Thrown by `postMessage` when given a `kind` starting with `ely:`. */
  export class ReservedMessageKindError extends Error {
    constructor(kind: string);
  }

  /** Thrown by `postMessage`/`requestExit`/`terminate` when the target id
   * is not a live process. */
  export class ProcessNotFoundError extends Error {
    constructor(id: number);
  }

  /** This process's own id. */
  export function currentProcessId(): ProcessHandle;

  /** The argument passed to the `spawn` that started this process, or
   * absent for the init process. */
  export function currentArguments(): Option<unknown>;

  /** Starts a new process from the userland-virtual entry path `path`,
   * passing `args` (JSON-serialized) as its `currentArguments()`. Returns
   * the new id; it joins the schedule on the next frame. */
  export function spawn(path: string, args: Option<unknown>): ProcessHandle;

  /** Queues `message` for `target`, delivered at `target`'s next turn.
   * @throws {ReservedMessageKindError} if `message.kind` starts with `ely:`.
   * @throws {ProcessNotFoundError} if `target` is not a live process. */
  export function postMessage(target: ProcessHandle, message: Message): void;

  /** An id returned by `addMessageHandler`, for `removeMessageHandler`. */
  export type MessageHandlerId = number;

  /** Registers `handler` for messages sent to this process, returning an
   * id for `removeMessageHandler`. Handlers all fire, in registration
   * order. Messages that arrive before the first handler is added are
   * queued, not lost. While at least one handler is registered the
   * process is kept alive; remove them all (or call `exit()`) to let it
   * be reaped. */
  export function addMessageHandler(
    handler: (envelope: Envelope) => void,
  ): MessageHandlerId;

  /** Unregisters a handler added by `addMessageHandler`. */
  export function removeMessageHandler(id: MessageHandlerId): void;

  /** Asks `target` to exit: delivers `{ kind: "ely:exit" }` and starts a
   * grace period after which the kernel force-reaps it. Cooperative
   * programs respond by clearing their handlers (or calling `exit()`).
   * @throws {ProcessNotFoundError} if `target` is not a live process. */
  export function requestExit(target: ProcessHandle): void;

  /** Drops `target` at the end of the current frame, no grace period. Its
   * `finally` blocks do not run.
   * @throws {ProcessNotFoundError} if `target` is not a live process. */
  export function terminate(target: ProcessHandle): void;

  /** Ends this process: the kernel reaps it at the end of this turn,
   * whatever work is still pending. */
  export function exit(): void;
}

/** What a JSX expression's `type`/`h`'s first argument can be: a tag name,
 * or a component function. */
type VNodeType = string | ((props: Props) => JSX.Element);

/** A JSX element's props, keyed by attribute name. */
interface Props {
  [key: string]: unknown;
}

// There's no DOM/React here, so this doesn't attempt to type individual
// intrinsic elements (`<div>`, `<span>`, ...) against a known element
// catalog — any tag name is accepted, with any props.
declare namespace JSX {
  /** What `h(...)`/JSX expressions evaluate to. */
  interface Element {
    type: VNodeType;
    props: Props;
    children: (Element | string | number)[];
  }

  interface IntrinsicElements {
    [tagName: string]: Record<string, unknown>;
  }
}

// `h`/`Fragment` are available as globals, so a program never needs to
// write `import { h, Fragment } from "jsx"` for JSX to work.
declare const h: (
  type: VNodeType,
  props: Props | null,
  ...children: unknown[]
) => JSX.Element;
declare const Fragment: (props: Props) => JSX.Element;
