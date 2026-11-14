# Grouping

Every error event belongs to exactly one issue of its project. The issue is
found by a hash computed from the event; events with the same hash share an
issue. The rules below are grouping version `v1`, stored with each issue
(`issues.grouping_version`) and with each event (`grouping` in the event
document, including the strings that were hashed).

## What is hashed

The first rule that applies decides:

1. **Fingerprint.** If the SDK sent a `fingerprint`, its entries are hashed.
   `{{ default }}` (spaces optional) expands to the components of the
   default grouping below, so `["{{ default }}", "tenant-a"]` splits the
   default issue by tenant. `{{ transaction }}`, `{{ level }}`,
   `{{ type }}` (exception type) and `{{ message }}` are filled in as well.
2. **Exception.** For every exception of the chain: its type, and its in-app
   frames (all frames when none is in-app). Each frame contributes its
   module, or else the base name of its file, plus its function name. When
   an exception has no frames, its value takes their place, with variable
   parts replaced (see 5).
3. **Stack trace.** For events without an exception: the in-app frames of
   the crashed thread, else the current thread, else the first thread.
4. **CSP report.** The effective directive and the host of the blocked URI.
5. **Message.** The message with variable parts replaced: e-mail
   addresses, URLs, UUIDs, IP addresses, hexadecimal and mixed
   letter-digit words, paths, dates, numbers and quoted strings.
6. Otherwise the title.

The hash is the xxh3-128 of `v1` and the components, written as 32 hex
digits.

## What does not split issues

- Line and column numbers: a deploy that moves code keeps its issues.
- Exception values when frames exist: `x is undefined` and
  `y is undefined` thrown at the same place are one issue.
- Build hashes in file names: `app.3f2a9c1e.js`, `index-BkL2q9Xz.js` and
  `main.1a2b3c4d.chunk.js` group as `app.js`, `index.js`, `main.chunk.js`.
  Query strings and fragments are ignored.
- Generated function name parts: `lambda$12`, `$$Lambda$…`, closure
  numbers, addresses (`0x7ff…`), `Object.` and `new ` prefixes; `?` and
  `<anonymous>` count as no name.
- Numbers, ids and other variable text in messages.

Frames are matched after symbolication, so minified names do not split
issues once source maps are uploaded (and an upload changes the hash of
issues seen before the upload).

## Title, culprit and metadata

- Exceptions: `Type: first line of the value` (200 characters), culprit is
  the transaction or the top in-app frame as `function(file)`.
- Type-less exceptions (messages sent with a stack trace): the message.
- CSP: `Blocked 'script' from 'evil.example'`.
- Messages: the first line.

`issues.metadata` holds `type`, `value`, `filename` and `function` of the
top frame for the issue list. Title and culprit are set when the issue is
created and do not change with later events.

## Limits

- There is no merging or splitting of issues after the fact, and no
  per-project grouping rules besides the SDK fingerprint.
- Changing the rules would need a new version; events grouped with `v1`
  keep their issues.
