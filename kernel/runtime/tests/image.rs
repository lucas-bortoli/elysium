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

#[test]
fn drawing_part_of_an_image_resized_flipped_or_turned_succeeds() {
    let runtime = eval(
        "import { loadImage } from 'ely:image'; \
         import { addDrawHandler, drawImage, drawImageRotated } from 'ely:framebuffer'; \
         globalThis.error = ''; \
         const image = loadImage('/test.png'); \
         addDrawHandler(() => { \
             try { \
                 drawImage(image, 0, 0, { sx: 1, sy: 1, sw: 2, sh: 2 }); \
                 drawImage(image, 0, 0, { scale: 3 }); \
                 drawImage(image, 0, 0, { scale: { x: 2, y: 4 } }); \
                 drawImage(image, 0, 0, { flipX: true, flipY: true }); \
                 drawImage(image.id, 0, 0, { sx: 1, scale: 2, flipX: true }); \
                 drawImageRotated(image, 20, 20, 0.7); \
                 drawImageRotated(image, 20, 20, 0.7, \
                                  { originX: 4, originY: 4, scale: 2 }); \
                 globalThis.error = 'none'; \
             } catch (err) { globalThis.error = String(err); } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn drawing_a_transformed_image_with_an_unknown_id_throws() {
    let runtime = eval(
        "import { addDrawHandler, drawImageRotated } from 'ely:framebuffer'; \
         globalThis.threw = false; \
         addDrawHandler(() => { \
             try { drawImageRotated(999999, 0, 0, 1); } \
             catch (err) { globalThis.threw = err instanceof TypeError; } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn asking_for_no_part_of_an_image_draws_nothing() {
    let runtime = eval(
        "import { loadImage } from 'ely:image'; \
         import { addDrawHandler, drawImage } from 'ely:framebuffer'; \
         globalThis.error = ''; \
         const image = loadImage('/test.png'); \
         addDrawHandler(() => { \
             try { \
                 drawImage(image, 0, 0, { sw: 0, sh: 0 }); \
                 drawImage(image, 0, 0, { sw: -5 }); \
                 globalThis.error = 'none'; \
             } catch (err) { globalThis.error = String(err); } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<String>(&runtime, "error"), "none");
}
