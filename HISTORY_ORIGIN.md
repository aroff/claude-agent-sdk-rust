# Retain native history origin

2026-10-25. The existing history reader now preserves the transcript entry's
`origin` as optional opaque JSON on `SessionMessage`. The real retained Linux
Claude 2.1.308 feedback row `8c768b8e-b942-4cf4-b4d2-2a152bf1dfce` has
`{"kind":"plugin","name":"aikit-codelaya-f32478fb"}`; the reader previously
dropped this field while retaining its UUID and content.

This reuses the existing file/store selection and conversion paths. It adds no
parser, transport, side effect, write API or model invocation. Null or missing
origin stays unknown; arbitrary future/malformed values remain opaque. Nothing
here authenticates the file or proves acknowledgement/consumption. Do not treat
an absent origin as owner authorship or promote an uncertain review receipt.

Old serialized `SessionMessage` objects still deserialize; `None` is omitted on
serialization. Rust callers constructing this public struct literally must add
`origin: None` (or their retained value). This is a Rust history metadata extension,
not a new field claimed for Python/live model-message parity. Native `message`
content and history filtering/branch/pagination semantics remain unchanged.

Qualification uses isolated Linux Rust 1.88 with the consumer's cached UUID
1.26.1 in a retained, untracked test lock. The first unconstrained resolution
selected UUID 1.27 requiring Rust 1.89; a trial 1.23 selection was unavailable
offline. Those preparation failures change no repository dependency constraints.
Windows provider checks remain deferred. AIKit's canonical adapter and read-only
checkpoint lookup consume this field separately from payload matching.
