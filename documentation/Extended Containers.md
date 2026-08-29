`ely:container` is a small standard library of data-holding types that
smooth over rough edges in plain JavaScript.

# Optional values

> JavaScript has two ways to say a value is absent: `null` and `undefined` and
> library code disagrees about which one it uses, which forces every caller
> to check for both.
>
> `ely:container` settles on one convention:
>
> - an absent value is always `undefined`, produced by calling `none()`;
> - a present value is just the value itself, wrapped by calling `some()`.

```ts
import { getOrElse, hasValue, map, none, some, unwrap } from "ely:container";

function findSprite(name: string): Option<Image> {
  return spriteCache.has(name) ? some(spriteCache.get(name)!) : none();
}

const sprite = findSprite("player");
if (hasValue(sprite)) {
  drawImage(sprite, 0, 0);
}
```

`ely:container` exports one function per operation on an `Option<T>`:

- `hasValue(option)`: a narrowing check. Tells TypeScript that an
  `Option<T>` which passes it is a real `T` for the rest of that scope, the
  same way checking `!== null` would, but written once and shared
  everywhere instead of every caller writing its own null check.
- `isOption(value)`: the reverse direction. Every value is a valid
  `Option<T>`, so it exists only to bring an `unknown` into `Option<T>`'s
  type at a boundary where nothing more specific is known yet.
- `getOrElse(option, fallback)`: reads a value out with a default
  standing in for absence.
- `map(option, fn)`: transforms a present value, passing an absence
  through untouched.
- `unwrap(option)`: returns the contained value if present, or throws
  `OptionUnwrapError` if not. The one function that turns absence into a
  thrown error rather than handling it, for the places where an absent
  value would mean a bug rather than a legitimate case to handle.
