# The examples browser

Elysium boots into a menu of example programs. Each one is a real program in
its own process ([1]), not a mode or a screen inside the menu, so what the
gallery demonstrates is not only the drawing machinery ([2]) but the fact
that a program can hand the screen to another program and take it back.

The init program the kernel starts at boot does almost nothing: it spawns
the browser. Nothing clears the screen unless a program asks, and the
browser clears it at the top of every redraw, so the first thing on screen
is the menu on a clean frame.

## Handing the screen over

When you pick an example, the browser spawns it and stops drawing — its
update ticker keeps running, watching for the way back, but doesn't put
anything on the screen while a child is live. That is the whole mechanism:
nothing clears the screen automatically, so the example's drawing is all
that's left on it, and the browser stays alive because a process with a
ticker still registered has work to do.

Pressing Escape terminates the example and the browser starts drawing the
menu again. Termination is immediate rather than a request to exit, because an
example asked politely to leave would go on drawing over the menu for the
whole of its grace period. The browser also notices an example that ends on
its own, by asking each frame whether it is still running, so an example that
exits or faults returns you to the menu the same way Escape does.

## Why Escape is a convention

There is no input focus in Elysium. There is one keyboard, and every running
program sees the same key press in the same frame — so which program a press
"belongs to" is not something the kernel can decide. Escape works because
every program in the gallery agrees about it: the browser acts on Escape only
while an example is running, and no example reads it at all. Both halves of
that are needed. An example that watched Escape would be reacting to the same
press that closes it, and a browser that watched it always would have no way
to tell "close this example" from a stray press at the menu.

This is worth knowing before writing a program that expects to own a key,
because the same reasoning applies to any of them.

## Adding an example

An example is a directory under `/programs/examples` containing an
`index.ts` and an `example.json`:

```json
{
  "title": "Shapes",
  "description": "Every filled and outlined shape a surface draws.",
  "order": 1
}
```

The browser finds examples by listing that directory and reading each
manifest, so a new example needs no edit to the browser itself. Entries are
ordered by `order`, then by title, because a directory listing has no order
worth relying on. A directory whose manifest is missing, unparseable or
untitled is skipped with a note on the console — one broken example costs you
that example rather than the gallery.

An example should draw a full screen of its own, leave Escape alone, and
stand on its own in one file. That last one is a deliberate constraint: an
example that reaches into a shared helper library stops being readable as a
single thing, which is most of what an example is for.

# References

[1] [Multitasking: many programs, one kernel](Multitasking.md)

[2] [Graphics](Graphics.md)

[3] [Coordinates](Coordinates.md)

[4] [Input](Input.md)
