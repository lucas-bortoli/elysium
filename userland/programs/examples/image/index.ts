// Loading a PNG and drawing it. A loaded image is palette-quantized once, at
// load time — see documentation/Image.md — so what's on screen is still only
// ever a palette color, the same promise every other drawing call keeps.
//
// photo.png is by Elizabeth Ferreira — see CREDIT.md.
//
// This example never reads Escape — that key belongs to the menu.

import { Color, screen } from "ely:graphics";
import { addUpdateTicker } from "ely:lifecycle";
import { loadImage } from "ely:image";

const photo = loadImage(`${import.meta.directoryName}/photo.png`);

const SCALE = 0.5;
const LEFT_X = 90;
const RIGHT_X = 430;
const IMAGE_Y = 90;

// The right half of the photo, at its natural size — a source rect crops to
// it without ever loading a second copy.
const CROP = { sx: photo.width / 2, sy: 0, sw: photo.width / 2, sh: photo.height };

const CREDIT = "photo by Elizabeth Ferreira";
const CREDIT_HEIGHT = screen.measureText(CREDIT).height;

addUpdateTicker(() => {
  screen.clear(Color.Slate900);
  screen.drawText(screen.width / 2, 6, "Image", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  screen.drawImage(photo, LEFT_X, IMAGE_Y, { scale: SCALE });
  screen.drawText(
    LEFT_X + (photo.width * SCALE) / 2,
    IMAGE_Y + photo.height * SCALE + 8,
    "drawImage, scaled",
    Color.Slate500,
    { align: "center" },
  );

  screen.drawImage(photo, RIGHT_X, IMAGE_Y, { ...CROP, scale: SCALE, flipX: true });
  screen.drawText(
    RIGHT_X + (CROP.sw * SCALE) / 2,
    IMAGE_Y + CROP.sh * SCALE + 8,
    "a cropped source rect, flipped",
    Color.Slate500,
    { align: "center" },
  );

  screen.drawText(screen.width - 6, screen.height - CREDIT_HEIGHT - 6, CREDIT, Color.Slate700, {
    align: "right",
  });
});
