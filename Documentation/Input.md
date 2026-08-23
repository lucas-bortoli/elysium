# Input

Elysium exposes the pointing device to programs through `ely:input`. The
pointer has one button — there's no secondary or middle button to check —
plus a scroll wheel.

Unlike the Framebuffer's draw handler, reading input isn't gated behind a
registered callback: a program can call any of `ely:input`'s functions from
wherever it likes — a draw handler, an update ticker ([1]), a timer — and
always gets the pointer's current state.

`getPointerPosition()` (and its `getPointerX`/`getPointerY` halves) report
the pointer's position in the same logical coordinate space the Framebuffer
draws in ([2]), not the window's physical pixels — a program never has to
think about the window's actual size or scale to line up what it draws with
where the pointer is.

The button has two different questions a program can ask about it, because
they answer different things. `isPointerDown()`/`isPointerUp()` report
whether the button is currently held, which is what a "press and hold to
keep drawing" tool wants. `wasPointerPressed()`/`wasPointerReleased()`
instead report whether the button's state *changed* on this frame — true
for exactly one frame per press or release, however long the button then
stays held — which is what a single click, like opening a menu, wants;
checking `isPointerDown()` for that would fire again every frame the button
happens to stay down. `getPointerDelta()` and `getScrollDelta()` follow the
same per-frame accounting: movement and scroll accumulated since the
previous frame, then reset.

```ts
import { Color, addDrawHandler, clearScreen, fillRectangle } from "ely:framebuffer";
import { getPointerPosition, isPointerDown } from "ely:input";

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  const { x, y } = getPointerPosition();
  const color = isPointerDown() ? Color.Amber400 : Color.Slate600;
  fillRectangle(x - 25, y - 25, 50, 50, color);
});
```

# References

[1] [Per-frame ticking](Lifecycle.md)

[2] [The Framebuffer](Framebuffer.md)
