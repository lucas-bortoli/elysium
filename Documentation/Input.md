# Input

Elysium exposes the pointing device and the keyboard to programs through
`ely:input`. The pointer has one button — there's no secondary or middle
button to check — plus a scroll wheel.

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

The keyboard follows the same polling model, and the same down/up versus
pressed/released distinction, but for keys instead of a single button.
Every key a program can ask about is one of `Key`'s named entries — never a
raw, unconstrained key code — identified by where it sits on the keyboard
rather than what it prints: `Key.KeyW` is the key in the "W" position on a
US layout, whatever an AZERTY keyboard happens to label it. That's the
right choice for the "WASD" kind of movement keys a game cares about, where
the position matters more than the character; a program that wants the
actual character typed, subject to the active layout and any modifier keys
held, isn't what `ely:input` is for.

`isKeyDown()`/`isKeyUp()` report whether a key is currently held, and
`wasKeyPressed()`/`wasKeyReleased()` report whether it changed state this
frame — exactly the pointer's distinction, applied per key.

```ts
import { addUpdateTicker, getDeltaTime } from "ely:lifecycle";
import { Key, isKeyDown } from "ely:input";

let x = 0;
addUpdateTicker(() => {
  const speed = 100; // pixels per second
  if (isKeyDown(Key.ArrowLeft)) x -= speed * getDeltaTime();
  if (isKeyDown(Key.ArrowRight)) x += speed * getDeltaTime();
});
```

# References

[1] [Per-frame ticking](Lifecycle.md)

[2] [The Framebuffer](Framebuffer.md)
