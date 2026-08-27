// A Rust-flavored `Option<T>`, fighting a little of JavaScript's null/undefined
// duality by giving userland one canonical "absent" representation
// (`undefined`, via `none()`) and a small set of combinators around it.

/** A value that may be absent: either a `T`, or `undefined`/`null` standing
 * in for "nothing". Mirrors Rust's `Option<T>`, adapted to accept either of
 * JavaScript's two nullish values rather than defining a single new one. */
export type Option<T> = T | undefined | null;

/** A total predicate: any value is a valid `Option<T>` (either `T` itself
 * or absent). Exists to narrow an `unknown` into `Option<T>` in generic
 * code; pairs with `hasValue` for the complementary "is it actually there"
 * check. */
export function isOption<T>(arg: unknown): arg is Option<T> {
  return true;
}

/** Narrows `Option<T>` down to a present `T`. */
export function hasValue<T>(arg: Option<T>): arg is T {
  return arg !== null && arg !== undefined;
}

/** The canonical absent `Option<T>`, always `undefined`. */
export function none<T>(): Option<T> {
  return undefined;
}

/** Wraps a definite value as a present `Option<T>`. */
export function some<T>(value: T): Option<T> {
  return value;
}

/** Returns `arg` if present, else `fallback`. */
export function getOrElse<T>(arg: Option<T>, fallback: T): T {
  return hasValue(arg) ? arg : fallback;
}

/** Transforms the contained value if present; passes an absence through
 * unchanged. */
export function map<T, U>(arg: Option<T>, fn: (value: T) => U): Option<U> {
  return hasValue(arg) ? fn(arg) : (arg as undefined | null);
}

/** Returns the contained value, or throws `OptionUnwrapError` if absent.
 * @throws {OptionUnwrapError} if `arg` is absent. */
export function unwrap<T>(arg: Option<T>): T {
  if (!hasValue(arg)) {
    throw new OptionUnwrapError("unwrap called on an empty Option");
  }
  return arg;
}

/** Thrown by `unwrap` when the `Option<T>` it's given is absent. */
export class OptionUnwrapError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "OptionUnwrapError";
  }
}
