# Bundled bitmap fonts

## cozette.bdf

Cozette, a 6x13 bitmap programming font.

- Upstream: <https://github.com/slavfox/Cozette>
- Copyright (c) 2020-2025 Ines <ines@moonwit.ch> (as recorded in the BDF header)
- License: MIT — see [LICENSE-cozette](LICENSE-cozette)

`cozette.bdf` is a FontForge BDF export of the upstream font, vendored here
unmodified. Only the ASCII printable range (U+0020..U+007E) is baked into the
kernel; see `build/fonts.rs` for the codepoint selection and
`kernel/text.rs` for how the generated data is used.
