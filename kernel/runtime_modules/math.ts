import type { Vector2d, Rectangle } from "ely:math";

/** Tolerance `floatEquals`/`vector2Equals` compare within, scaled by the
 * magnitude of the values being compared — coarser than `Number.EPSILON`,
 * whose ~2.22e-16 gap near `1.0` is too tight once error has accumulated
 * over a few float operations. */
export const EPSILON = 0.000001;

/** Whether `a` and `b` are equal within `EPSILON`, scaled by their magnitude. */
export function floatEquals(a: number, b: number): boolean {
  return Math.abs(a - b) <= EPSILON * Math.max(1, Math.abs(a), Math.abs(b));
}

/** The vector `{ x: 0, y: 0 }`. */
export function vector2Zero(): Vector2d {
  return { x: 0, y: 0 };
}

/** The vector `{ x: 1, y: 1 }`. */
export function vector2One(): Vector2d {
  return { x: 1, y: 1 };
}

/** Whether `a` and `b` are equal within `EPSILON`, component-wise. */
export function vector2Equals(a: Vector2d, b: Vector2d): boolean {
  return floatEquals(a.x, b.x) && floatEquals(a.y, b.y);
}

/** Adds `a` and `b` component-wise. */
export function vector2Add(a: Vector2d, b: Vector2d): Vector2d {
  return { x: a.x + b.x, y: a.y + b.y };
}

/** Adds `value` to both of `v`'s components. */
export function vector2AddValue(v: Vector2d, value: number): Vector2d {
  return { x: v.x + value, y: v.y + value };
}

/** Subtracts `b` from `a` component-wise. */
export function vector2Subtract(a: Vector2d, b: Vector2d): Vector2d {
  return { x: a.x - b.x, y: a.y - b.y };
}

/** Subtracts `value` from both of `v`'s components. */
export function vector2SubtractValue(v: Vector2d, value: number): Vector2d {
  return { x: v.x - value, y: v.y - value };
}

/** Scales `v` by `factor`. */
export function vector2Scale(v: Vector2d, factor: number): Vector2d {
  return { x: v.x * factor, y: v.y * factor };
}

/** The straight-line distance between `a` and `b`. */
export function vector2Distance(a: Vector2d, b: Vector2d): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

/** The point `t` of the way from `a` to `b` (`t = 0` is `a`, `t = 1` is `b`). */
export function vector2Lerp(a: Vector2d, b: Vector2d, t: number): Vector2d {
  return { x: lerp(a.x, b.x, t), y: lerp(a.y, b.y, t) };
}

/** `v` with both components negated. */
export function vector2Negate(v: Vector2d): Vector2d {
  return { x: -v.x, y: -v.y };
}

/** The length of `v`. */
export function vector2Length(v: Vector2d): number {
  return Math.hypot(v.x, v.y);
}

/** The squared length of `v`, cheaper than `vector2Length` when only comparing magnitudes. */
export function vector2LengthSquared(v: Vector2d): number {
  return v.x * v.x + v.y * v.y;
}

/** The dot product of `a` and `b`. */
export function vector2Dot(a: Vector2d, b: Vector2d): number {
  return a.x * b.x + a.y * b.y;
}

/** `v` scaled to length 1, or `vector2Zero()` if `v` has zero length. */
export function vector2Normalize(v: Vector2d): Vector2d {
  const length = vector2Length(v);
  return length === 0 ? vector2Zero() : vector2Scale(v, 1 / length);
}

/** `v` rotated by `radians` around the origin. */
export function vector2Rotate(v: Vector2d, radians: number): Vector2d {
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  return { x: v.x * cos - v.y * sin, y: v.x * sin + v.y * cos };
}

/** `v` with each component restricted to the `[min, max]` range. */
export function vector2Clamp(v: Vector2d, min: Vector2d, max: Vector2d): Vector2d {
  return { x: clamp(v.x, min.x, max.x), y: clamp(v.y, min.y, max.y) };
}

/** The angle of `v`, in radians, measured counterclockwise from the positive x-axis. */
export function vector2Angle(v: Vector2d): number {
  return Math.atan2(v.y, v.x);
}

/** Whether `a` and `b` overlap by any amount. */
export function rectangleIntersects(a: Rectangle, b: Rectangle): boolean {
  return (
    a.x < b.x + b.width &&
    a.x + a.width > b.x &&
    a.y < b.y + b.height &&
    a.y + a.height > b.y
  );
}

/** Whether `point` falls inside `rect`. */
export function rectangleContains(rect: Rectangle, point: Vector2d): boolean {
  return (
    point.x >= rect.x &&
    point.x < rect.x + rect.width &&
    point.y >= rect.y &&
    point.y < rect.y + rect.height
  );
}

/** `value`, restricted to the `[min, max]` range. */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/** The value `t` of the way from `a` to `b` (`t = 0` is `a`, `t = 1` is `b`). */
export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** `degrees` converted to radians. */
export function degToRad(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

/** `radians` converted to degrees. */
export function radToDeg(radians: number): number {
  return (radians * 180) / Math.PI;
}
