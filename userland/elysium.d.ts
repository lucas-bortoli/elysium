// Ambient declarations for the Elysium host API: globals (print, timers,
// the JSX factory), the ambient `JSX` namespace, and the `ely:framebuffer`/
// `ely:lifecycle`/`ely:math`/`ely:input`/`ely:image` namespaces.

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

  /** Thrown by `clearScreen`/`fillRectangle`/`drawImage` when called from
   * outside a currently running draw handler (see `addDrawHandler`). */
  export class DrawOutsideHandlerError extends Error {
    constructor();
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

  /** Draws `image` with its top-left corner at `(x, y)`, at its natural
   * size — no scaling or rotation. */
  export function drawImage(
    image: import("ely:image").Image | import("ely:image").ImageId,
    x: number,
    y: number,
  ): void;

  /** Sets how many physical pixels the window draws each logical pixel as
   * — an integer of at least 1. Takes effect on the next frame; unlike
   * `clearScreen`/`fillRectangle`, can be called from anywhere, not just
   * from inside a draw handler. */
  export function setScale(scale: number): void;
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

  /** Whether `a` and `b` overlap by any amount. */
  export function rectangleIntersects(a: Rectangle, b: Rectangle): boolean;

  /** Whether `point` falls inside `rect`. */
  export function rectangleContains(rect: Rectangle, point: Vector2d): boolean;

  /** `value`, restricted to the `[min, max]` range. */
  export function clamp(value: number, min: number, max: number): number;

  /** The value `t` of the way from `a` to `b` (`t = 0` is `a`, `t = 1` is `b`). */
  export function lerp(a: number, b: number, t: number): number;
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
   * program's own directory, or isn't a decodable PNG. */
  export class ImageLoadError extends Error {
    constructor(message: string);
  }

  /** Loads the PNG at `path`, resolved relative to the program's own root
   * directory. */
  export function loadImage(path: string): Image;

  /** Frees a loaded image early. An image not explicitly unloaded stays
   * loaded for the lifetime of the program, not until nothing references
   * it anymore — dropping every reference to an `Image` does not free it. */
  export function unloadImage(image: Image | ImageId): void;
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
