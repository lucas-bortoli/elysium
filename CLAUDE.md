Elysium OS is a fantasy operating system based on Rust and JavaScript.

Its features are:

- Each program runs in an isolated JavaScript virtual machine, the Rust kernel acting as an hypervisor
- The Rust fantasy kernel provides drawing, input and sound machinery
- Programs may communicate with each other through message-passing

## Writing documentation

Docs under `documentation/` are about Elysium as a system — its architecture,
mechanisms, and the contracts programs and the kernel rely on — not about the
Rust code that happens to implement it today. Write for a reader who wants to
understand how Elysium behaves, not one reading the source alongside it.
Describe mechanisms conceptually, the way you'd explain them out loud: a hook
the engine checks periodically, not `Runtime::set_interrupt_handler`; what
happens and why, not which struct, function, or crate it's wired through.
Rust type names, crate/API names, and internal function or struct names
(`Rc<Cell<...>>`, `GuardedError`, `rquickjs::Error`) change on every refactor
and mean nothing to a reader thinking about Elysium's behavior rather than
its implementation, so leave them out; it's fine to name a source file once
if it helps someone find the code, but don't narrate the implementation
through its symbols. Prefer describing a guarantee or contract — a timed-out
VM is destroyed, the kernel keeps running — over describing the code path
that provides it.

Write in prose, in paragraphs, the way this file and the rest of
`documentation/` are written. Reach for a bullet list only when you're
actually enumerating discrete, parallel items (a set of options, a
non-goals list); don't default to header-per-topic, bullet-per-sentence
structure for material that's really just an explanation — that fragments
an argument into disconnected fragments and makes the reader do the work of
reassembling it. A short document doesn't need section headers at all; a
longer one should still read as connected reasoning under each heading, not
a list of one-liners.

That default is about explanation, and some documents are also references.
A surface with a large closed set of knobs — every waveform, every option
with its default and its range, every error and what triggers it — is
material a reader scans and comes back to rather than follows once, and a
table is the right tool for it; `documentation/Sound.md` is the model.
The test is what the material actually is, not which document it's in: a
table for a closed set, prose for an argument. Two options contrasted to
explain who owns a decision are an argument, and splitting them into cells
makes the reader assemble the reasoning themselves — while ten options
with their defaults are a set, and writing them out as sentences buries
what someone opened the page to look up. A reference doc still explains
its mechanisms in prose; the tables hang off that explanation rather than
replacing it.

## Writing code comments

A comment should describe the code as it stands, not the code it used to be
or an alternative that was never taken. Watch for the "X, rather than Y" /
"X, instead of Y" shape: if Y is a real hazard the reader needs ruled out —
an ASI break, a use-after-free, a race — keeping the contrast earns its
place. If Y is just a design not chosen, drop it and state what the code
does; a reader who never knew Y was considered loses nothing, and the
comment stops rotting the moment the history it references is forgotten.

## Writing tests

Tests live at one of two layers. A test that exercises a device's own
internals, with no JavaScript involved, goes in a `mod tests` inside that
device's own module — the clip and transform algebra, path geometry, glyph
metrics, how window events fold into input state. A test that exercises an
`ely:` surface the way a program sees it, with JavaScript as the input, goes
in `kernel/runtime/tests/`, one module per surface, using the shared VM
helpers in `kernel/runtime/tests.rs`.

The two are complementary rather than alternatives, and a mechanism worth
having is usually worth covering at both layers. Prefer the inner one when
either would do: it runs without building a VM, and it fails pointing at the
thing that broke.

Name a test as a sentence about the behaviour it pins down
(`a_nested_clip_cannot_widen_the_one_it_nests_in`), not after the function it
calls.

## Committing

When asked to commit, split the changes into separate commits along
logical lines rather than one commit for everything in the working tree —
e.g. a behavioral/code change as one commit, and documentation added or
updated alongside it as another. Each commit should stand on its own as a
coherent, reviewable unit.

Commit subject lines must follow Conventional Commits:
`type(optional scope): description`, lowercase, imperative mood, no
trailing period — e.g. `feat: guard every call into a VM with a
cooperative timeout`, `docs(transform): explain the jsx/type-stripping
pipeline`, `fix: resolve embedded modules before disk paths`. Use `feat`
for new behavior, `fix` for bug fixes, `docs` for documentation-only
changes, `refactor` for internal restructuring with no behavior change,
`test` for test-only changes, and `chore` for everything else
(dependency bumps, tooling). Add a scope in parentheses when a commit is
narrowly about one area (`runtime`, `transform`, etc.) and omit it when
the commit already reads clearly without one.
