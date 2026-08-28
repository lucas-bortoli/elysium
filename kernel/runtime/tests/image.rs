//! The `ely:image` surface: `loadImage`/`drawImage` and the errors loading can
//! raise.

use super::*;

#[test]
fn load_image_and_draw_image_round_trip_without_throwing() {
    let runtime = eval(
        "import { loadImage } from 'ely:image'; \
         import { addDrawHandler, drawImage } from 'ely:framebuffer'; \
         globalThis.drawn = false; \
         const image = loadImage('/test.png'); \
         addDrawHandler(() => { drawImage(image, 10, 10); globalThis.drawn = true; });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "drawn"));
}

#[test]
fn draw_image_with_an_unknown_id_throws() {
    let runtime = eval(
        "import { addDrawHandler, drawImage } from 'ely:framebuffer'; \
         globalThis.threw = false; \
         addDrawHandler(() => { \
             try { drawImage(999999, 0, 0); } catch { globalThis.threw = true; } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn load_image_outside_userland_root_throws_image_load_error() {
    let runtime = eval(
        "import { loadImage, ImageLoadError } from 'ely:image'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             loadImage('/../../framebuffer.rs'); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof ImageLoadError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}

#[test]
fn load_image_with_a_relative_path_throws_relative_path_error() {
    let runtime = eval(
        "import { loadImage } from 'ely:image'; \
         import { RelativePathError } from 'ely:filesystem'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             loadImage('test.png'); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof RelativePathError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}
