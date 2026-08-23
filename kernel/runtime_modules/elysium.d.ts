// Everything TypeScript needs to typecheck a program against the Elysium
// host API, in one place: ambient globals (print, timers, the JSX factory),
// the ambient `JSX` namespace, and virtual `"ely:*"` module specifiers.
// TypeScript has no file on disk to resolve a bare `"ely:framebuffer"`/
// `"ely:loop"` import against (they're schemes the kernel's module loader
// recognizes, backed at runtime by the matching .ts file under this
// directory), so the `declare module` blocks below are what let programs
// importing them typecheck at all. Keep in sync with the bindings
// registered in kernel/runtime.rs and kernel/timers.rs, and with
// kernel/runtime_modules/jsx-runtime.ts, framebuffer.ts, loop.ts, and
// kernel/framebuffer/colors.rs's `Color` enum.

/** Writes a line to the host's stdout. */
declare function print(...message: any): void;

// Ambient JSX namespace paired with the classic (`jsxFactory: "h"`) JSX
// transform configured in tsconfig.json and jsx-runtime.ts's `h`/`Fragment`.
// There's no DOM/React here, so this doesn't attempt to type individual
// intrinsic elements (`<div>`, `<span>`, ...) against a known element
// catalog — any tag name is accepted, with any props.
declare namespace JSX {
  /** What `h(...)`/JSX expressions evaluate to. */
  type Element = import("./jsx-runtime").VNode;

  interface IntrinsicElements {
    [tagName: string]: Record<string, unknown>;
  }
}

// kernel/runtime.rs bootstraps jsx-runtime.ts's `h`/`Fragment` exports onto
// every program's global scope, so the classic JSX transform above can find
// them without a program ever writing `import { h, Fragment } from "jsx"`.
declare const h: (
  type: import("./jsx-runtime").VNodeType,
  props: import("./jsx-runtime").Props | null,
  ...children: unknown[]
) => JSX.Element;
declare const Fragment: (props: import("./jsx-runtime").Props) => JSX.Element;

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
  /** A color from the kernel's fixed palette, as one of the named constants below. */
  export type Color = number;

  /** Thrown by `clearScreen`/`fillRectangle` when called from outside a
   * currently running draw handler (see `addDrawHandler`). */
  export class DrawOutsideHandlerError extends Error {
    constructor();
  }

  export type DrawTickerId = number;

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

  export const RED_50: Color;
  export const RED_100: Color;
  export const RED_200: Color;
  export const RED_300: Color;
  export const RED_400: Color;
  export const RED_500: Color;
  export const RED_600: Color;
  export const RED_700: Color;
  export const RED_800: Color;
  export const RED_900: Color;
  export const RED_950: Color;
  export const ORANGE_50: Color;
  export const ORANGE_100: Color;
  export const ORANGE_200: Color;
  export const ORANGE_300: Color;
  export const ORANGE_400: Color;
  export const ORANGE_500: Color;
  export const ORANGE_600: Color;
  export const ORANGE_700: Color;
  export const ORANGE_800: Color;
  export const ORANGE_900: Color;
  export const ORANGE_950: Color;
  export const AMBER_50: Color;
  export const AMBER_100: Color;
  export const AMBER_200: Color;
  export const AMBER_300: Color;
  export const AMBER_400: Color;
  export const AMBER_500: Color;
  export const AMBER_600: Color;
  export const AMBER_700: Color;
  export const AMBER_800: Color;
  export const AMBER_900: Color;
  export const AMBER_950: Color;
  export const YELLOW_50: Color;
  export const YELLOW_100: Color;
  export const YELLOW_200: Color;
  export const YELLOW_300: Color;
  export const YELLOW_400: Color;
  export const YELLOW_500: Color;
  export const YELLOW_600: Color;
  export const YELLOW_700: Color;
  export const YELLOW_800: Color;
  export const YELLOW_900: Color;
  export const YELLOW_950: Color;
  export const LIME_50: Color;
  export const LIME_100: Color;
  export const LIME_200: Color;
  export const LIME_300: Color;
  export const LIME_400: Color;
  export const LIME_500: Color;
  export const LIME_600: Color;
  export const LIME_700: Color;
  export const LIME_800: Color;
  export const LIME_900: Color;
  export const LIME_950: Color;
  export const GREEN_50: Color;
  export const GREEN_100: Color;
  export const GREEN_200: Color;
  export const GREEN_300: Color;
  export const GREEN_400: Color;
  export const GREEN_500: Color;
  export const GREEN_600: Color;
  export const GREEN_700: Color;
  export const GREEN_800: Color;
  export const GREEN_900: Color;
  export const GREEN_950: Color;
  export const EMERALD_50: Color;
  export const EMERALD_100: Color;
  export const EMERALD_200: Color;
  export const EMERALD_300: Color;
  export const EMERALD_400: Color;
  export const EMERALD_500: Color;
  export const EMERALD_600: Color;
  export const EMERALD_700: Color;
  export const EMERALD_800: Color;
  export const EMERALD_900: Color;
  export const EMERALD_950: Color;
  export const TEAL_50: Color;
  export const TEAL_100: Color;
  export const TEAL_200: Color;
  export const TEAL_300: Color;
  export const TEAL_400: Color;
  export const TEAL_500: Color;
  export const TEAL_600: Color;
  export const TEAL_700: Color;
  export const TEAL_800: Color;
  export const TEAL_900: Color;
  export const TEAL_950: Color;
  export const CYAN_50: Color;
  export const CYAN_100: Color;
  export const CYAN_200: Color;
  export const CYAN_300: Color;
  export const CYAN_400: Color;
  export const CYAN_500: Color;
  export const CYAN_600: Color;
  export const CYAN_700: Color;
  export const CYAN_800: Color;
  export const CYAN_900: Color;
  export const CYAN_950: Color;
  export const SKY_50: Color;
  export const SKY_100: Color;
  export const SKY_200: Color;
  export const SKY_300: Color;
  export const SKY_400: Color;
  export const SKY_500: Color;
  export const SKY_600: Color;
  export const SKY_700: Color;
  export const SKY_800: Color;
  export const SKY_900: Color;
  export const SKY_950: Color;
  export const BLUE_50: Color;
  export const BLUE_100: Color;
  export const BLUE_200: Color;
  export const BLUE_300: Color;
  export const BLUE_400: Color;
  export const BLUE_500: Color;
  export const BLUE_600: Color;
  export const BLUE_700: Color;
  export const BLUE_800: Color;
  export const BLUE_900: Color;
  export const BLUE_950: Color;
  export const INDIGO_50: Color;
  export const INDIGO_100: Color;
  export const INDIGO_200: Color;
  export const INDIGO_300: Color;
  export const INDIGO_400: Color;
  export const INDIGO_500: Color;
  export const INDIGO_600: Color;
  export const INDIGO_700: Color;
  export const INDIGO_800: Color;
  export const INDIGO_900: Color;
  export const INDIGO_950: Color;
  export const VIOLET_50: Color;
  export const VIOLET_100: Color;
  export const VIOLET_200: Color;
  export const VIOLET_300: Color;
  export const VIOLET_400: Color;
  export const VIOLET_500: Color;
  export const VIOLET_600: Color;
  export const VIOLET_700: Color;
  export const VIOLET_800: Color;
  export const VIOLET_900: Color;
  export const VIOLET_950: Color;
  export const PURPLE_50: Color;
  export const PURPLE_100: Color;
  export const PURPLE_200: Color;
  export const PURPLE_300: Color;
  export const PURPLE_400: Color;
  export const PURPLE_500: Color;
  export const PURPLE_600: Color;
  export const PURPLE_700: Color;
  export const PURPLE_800: Color;
  export const PURPLE_900: Color;
  export const PURPLE_950: Color;
  export const FUCHSIA_50: Color;
  export const FUCHSIA_100: Color;
  export const FUCHSIA_200: Color;
  export const FUCHSIA_300: Color;
  export const FUCHSIA_400: Color;
  export const FUCHSIA_500: Color;
  export const FUCHSIA_600: Color;
  export const FUCHSIA_700: Color;
  export const FUCHSIA_800: Color;
  export const FUCHSIA_900: Color;
  export const FUCHSIA_950: Color;
  export const PINK_50: Color;
  export const PINK_100: Color;
  export const PINK_200: Color;
  export const PINK_300: Color;
  export const PINK_400: Color;
  export const PINK_500: Color;
  export const PINK_600: Color;
  export const PINK_700: Color;
  export const PINK_800: Color;
  export const PINK_900: Color;
  export const PINK_950: Color;
  export const ROSE_50: Color;
  export const ROSE_100: Color;
  export const ROSE_200: Color;
  export const ROSE_300: Color;
  export const ROSE_400: Color;
  export const ROSE_500: Color;
  export const ROSE_600: Color;
  export const ROSE_700: Color;
  export const ROSE_800: Color;
  export const ROSE_900: Color;
  export const ROSE_950: Color;
  export const SLATE_50: Color;
  export const SLATE_100: Color;
  export const SLATE_200: Color;
  export const SLATE_300: Color;
  export const SLATE_400: Color;
  export const SLATE_500: Color;
  export const SLATE_600: Color;
  export const SLATE_700: Color;
  export const SLATE_800: Color;
  export const SLATE_900: Color;
  export const SLATE_950: Color;
  export const GRAY_50: Color;
  export const GRAY_100: Color;
  export const GRAY_200: Color;
  export const GRAY_300: Color;
  export const GRAY_400: Color;
  export const GRAY_500: Color;
  export const GRAY_600: Color;
  export const GRAY_700: Color;
  export const GRAY_800: Color;
  export const GRAY_900: Color;
  export const GRAY_950: Color;
  export const ZINC_50: Color;
  export const ZINC_100: Color;
  export const ZINC_200: Color;
  export const ZINC_300: Color;
  export const ZINC_400: Color;
  export const ZINC_500: Color;
  export const ZINC_600: Color;
  export const ZINC_700: Color;
  export const ZINC_800: Color;
  export const ZINC_900: Color;
  export const ZINC_950: Color;
  export const NEUTRAL_50: Color;
  export const NEUTRAL_100: Color;
  export const NEUTRAL_200: Color;
  export const NEUTRAL_300: Color;
  export const NEUTRAL_400: Color;
  export const NEUTRAL_500: Color;
  export const NEUTRAL_600: Color;
  export const NEUTRAL_700: Color;
  export const NEUTRAL_800: Color;
  export const NEUTRAL_900: Color;
  export const NEUTRAL_950: Color;
  export const STONE_50: Color;
  export const STONE_100: Color;
  export const STONE_200: Color;
  export const STONE_300: Color;
  export const STONE_400: Color;
  export const STONE_500: Color;
  export const STONE_600: Color;
  export const STONE_700: Color;
  export const STONE_800: Color;
  export const STONE_900: Color;
  export const STONE_950: Color;
  export const TAUPE_50: Color;
  export const TAUPE_100: Color;
  export const TAUPE_200: Color;
  export const TAUPE_300: Color;
  export const TAUPE_400: Color;
  export const TAUPE_500: Color;
  export const TAUPE_600: Color;
  export const TAUPE_700: Color;
  export const TAUPE_800: Color;
  export const TAUPE_900: Color;
  export const TAUPE_950: Color;
  export const MAUVE_50: Color;
  export const MAUVE_100: Color;
  export const MAUVE_200: Color;
  export const MAUVE_300: Color;
  export const MAUVE_400: Color;
  export const MAUVE_500: Color;
  export const MAUVE_600: Color;
  export const MAUVE_700: Color;
  export const MAUVE_800: Color;
  export const MAUVE_900: Color;
  export const MAUVE_950: Color;
  export const MIST_50: Color;
  export const MIST_100: Color;
  export const MIST_200: Color;
  export const MIST_300: Color;
  export const MIST_400: Color;
  export const MIST_500: Color;
  export const MIST_600: Color;
  export const MIST_700: Color;
  export const MIST_800: Color;
  export const MIST_900: Color;
  export const MIST_950: Color;
  export const OLIVE_50: Color;
  export const OLIVE_100: Color;
  export const OLIVE_200: Color;
  export const OLIVE_300: Color;
  export const OLIVE_400: Color;
  export const OLIVE_500: Color;
  export const OLIVE_600: Color;
  export const OLIVE_700: Color;
  export const OLIVE_800: Color;
  export const OLIVE_900: Color;
  export const OLIVE_950: Color;
  export const BLACK: Color;
  export const WHITE: Color;
}

declare module "ely:loop" {
  export type TickerId = number;

  /** Registers `handler` to run once per frame with the time (in seconds)
   * since the previous frame. Returns an id for `removeUpdateTicker`. */
  export function addUpdateTicker(handler: (dt: number) => void): TickerId;

  /** Stops calling the ticker registered under `id`. */
  export function removeUpdateTicker(id: TickerId): void;

  /** The time, in seconds, since the previous frame. */
  export function getDeltaTime(): number;
}
