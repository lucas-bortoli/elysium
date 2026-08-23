// The Framebuffer device: a fixed-resolution GPU surface programs draw to.
// Colors are always one of `Color`'s named entries (the kernel's fixed
// palette), never raw RGBA channels a program could get wrong.

declare function __framebuffer_clear_screen(color: Color): void;
declare function __framebuffer_fill_rectangle(
  x: number,
  y: number,
  w: number,
  h: number,
  color: Color,
): void;

// The kernel's fixed, curated color palette. Every color a program can
// draw with is one of these named entries — never a raw, unconstrained
// RGBA value — generated together with kernel/framebuffer/colors.rs's
// `Color` enum so the ids the two sides agree on never drift.
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

// A color from the kernel's fixed palette, as one of `Color`'s named
// entries (e.g. `Color.Slate900`). The underlying numeric id has no
// meaning of its own outside matching kernel/framebuffer/colors.rs's
// `Color` enum.
export type Color = (typeof Color)[keyof typeof Color];

export class DrawOutsideHandlerError extends Error {
  constructor() {
    super(
      "clearScreen/fillRectangle can only be called from inside a registered draw handler",
    );
    this.name = "DrawOutsideHandlerError";
  }
}

export type DrawTickerId = number;

let nextHandlerId = 1;
const drawHandlers = new Map<DrawTickerId, () => void>();
let insideDrawHandler = false;
let frameScheduled = false;

function frame() {
  frameScheduled = false;
  insideDrawHandler = true;
  try {
    for (const handler of [...drawHandlers.values()]) handler();
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
