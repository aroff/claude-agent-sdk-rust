# Preserve visible history behind native control rows

## Failure

On the consumer-pinned `aa01a48`, `get_session_info` found a real Claude 2.1.308
session with 73 transcript entries, while `get_session_messages` returned none.
The transcript ended with an assistant, an attachment and system rows linked
through `parentUuid`. The last assistant was therefore not a graph leaf, and
the leaf selector accepted only non-meta user/assistant rows. No row qualified.
The same selector remains in main `94b0939` at investigation time.

## Change

Select the latest main conversation chain through control/meta rows, then keep
the existing visible-message filtering and pagination. Ignore sidechain/team
edges when selecting the main leaf. Skip orphan control chains with no visible
message rather than replacing the current conversation with an empty branch.
Meta-only sessions still expose no visible messages. No transcript is modified,
no CLI is spawned and no public type or method changes.

Three regressions fail on the unchanged implementation: linked native control
tails, non-main children hiding a main leaf, and orphan control rows causing an
older branch to be selected. All pass after the fix. Pagination and meta-only
filtering are covered in the third case.

## Validation

Linux Rust 1.88, offline/locked standalone dependency graph:

- Full `cargo test`: 187 passed, 5 live tests ignored. The real-transcript test
  has no owner history mounted and is not independent native acceptance.
- `cargo build --all-targets`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo fmt --check`: passed.

The first standalone resolution selected uuid 1.27, requiring Rust 1.89. The
qualification lock reuses uuid 1.26.1 from the existing consumer SDK lock; no
dependency requirement is changed. Required lint/format gates also exposed six
existing format-string lints and existing formatting drift. Those mechanical,
behavior-preserving changes are separated from the history fix in Git history.

AIKit's unchanged `HistoryReader` adapter, rebuilt against the fixed dependency,
returns 32 visible messages from the same native transcript and locates the exact
stored review UUID and full framed payload. The before/after transcript hashes
are identical. A meta-note-only transcript still returns zero visible messages.
An initial read made before the rebuild finished is retained as an invalid fixed
binary comparison; only the read after confirmed build completion is qualified.

This qualifies stored-history reading, not authenticated caller identity,
generation admission, native acknowledgement, main-loop consumption, durable
receipt authority or Windows integration. Raw synthetic consumer captures and
logs remain local; no owner credentials or private source are committed here.
