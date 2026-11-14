# Profiling

rupta stores the sampled profiles that the official open-source SDKs send
and shows them as flamegraphs, for one profile or merged over the recent
profiles of a transaction.

## Profile formats

Both formats contain `frames`, `stacks` (lists of frame indices, innermost
frame first), `samples` (a stack and a thread) and `thread_metadata`:

| Item | Sent by | Sample times | Link to transactions |
| --- | --- | --- | --- |
| `profile` (version 1, transaction profiling) | Python with `profiles_sample_rate`, PHP with `profiles_sample_rate` and the `excimer` extension, older JavaScript SDKs | `elapsed_since_start_ns` relative to the profile's `timestamp` | `transaction` (or `transactions[0]`): name, id, trace id, active thread |
| `profile_chunk` (version 2, continuous profiling) | `@sentry/profiling-node` (`profileSessionSampleRate`, `profileLifecycle`), Python with `profile_session_sample_rate`, the browser SDK with `browserProfilingIntegration` | absolute `timestamp` in seconds | `profiler_id`, also in the transaction's `contexts.profile.profiler_id` or the span attribute `sentry.profiler_id` |

The browser profiler uses the JS Self-Profiling API, which Chromium
enables only for documents served with the header
`Document-Policy: js-profiling`. The smoke test serves its page that way
and receives browser profile chunks.

## What is stored

The document is checked (it must parse as a sampled profile), compressed
with zstd and stored in the object store under
`profiles/{project}/{day}/{profile_id}.json.zst` (for chunks the chunk id
is the profile id). A row in `profiles` describes it: kind (`transaction`
or `chunk`), transaction name and id, trace id, profiler id, platform,
release, environment, start (first sample), duration, number of samples,
size, object key and day. Profiles up to the attachment limit (100 MiB)
are accepted.

## Flamegraphs

Flamegraphs are built when they are requested:

- One thread is shown: the transaction's thread (`active_thread_id`),
  else a thread named `main`/`MainThread`, else the thread with the most
  samples; `?thread=` chooses another.
- Each sample's stack is walked from the outermost frame; equal frames
  under the same parent are merged. `self_weight` counts the samples in
  which a node was the innermost frame, `total_weight` those in which it
  was on the stack. Weights are sample counts, not durations (the SDKs
  sample at a fixed rate, about 100 Hz).
- Children are in the order they were first sampled (time order). Sort
  them by `total_weight` for a left-heavy view.
- Frames are identified by function name, file and package. A frame's
  package comes from the `package` field or from its path
  (`node_modules/<pkg>`, `site-packages/<pkg>`, `vendor/<vendor>/<pkg>`,
  `node:` built-ins). `is_application` is the SDK's `in_app` flag, else
  true for frames without a package outside the standard library.
- Stacks deeper than 512 frames are cut at the root side.
- Recently built single-profile flamegraphs are kept in memory (64
  entries).

The aggregated flamegraph of a transaction merges the latest (at most 50)
profiles in the range: transaction profiles with that transaction name,
and for transactions recorded with continuous profiling, the samples of
the chunks of their profiler session that fall within the transaction's
start and end (chunks that started up to two minutes before the
transaction are considered).

## Retention and switches

`settings.retention.profiles` (days, default 30): rows and objects of
older days are deleted (objects by day prefix). `settings.data_types.profiles
= false` drops profiles (outcome `filtered`, reason `disabled`).

## API

Schemas under the `profiling` tag of `docs/api/openapi.json`.

- `GET /api/v1/orgs/{org}/profiles?transaction=&kind=&release=&profiler_id=&cursor=&limit=`
  plus the scope parameters: newest first,
  `[{profile_id, project_id, kind, transaction_name, transaction_id, trace_id, profiler_id, platform, release, environment, started, duration_ms, samples, size}]`.
- `GET /api/v1/orgs/{org}/profiles/{project}/{profile_id}/flamegraph?thread=&start=&end=`:
  `start`/`end` (Unix seconds) limit the samples to a time window, for
  example one transaction within a chunk.
- `GET /api/v1/orgs/{org}/profiles/{project}/{profile_id}/raw`: the
  document as the SDK sent it, as a JSON download.
- `GET /api/v1/orgs/{org}/profiles/aggregate-flamegraph?transaction=` plus
  the scope parameters (default range 7 days).

A flamegraph answer:

```json
{"frames": [{"name": "computeChecksum", "file": "file:///srv/app/node.mjs", "line": 42,
             "package": null, "is_application": true}],
 "root": {"frame_index": null, "self_weight": 0, "total_weight": 31,
          "children": [{"frame_index": 0, "self_weight": 3, "total_weight": 31, "children": []}]},
 "samples": 31, "profiles": 1, "thread_id": "0",
 "threads": [{"id": "0", "name": "main", "samples": 31}],
 "platform": "node", "profile_ids": ["d0e6cc3f4ecf4b6f9d04acb55b341ac4"]}
```

`thread_id` and `threads` are set for single profiles; `profile_ids`
lists the profiles that contributed samples.

## Tested

JavaScript (Node continuous profiling and browser profiling), Python
(transaction profiles and continuous profiling): captured from the SDKs
and replayed in tests. PHP profiles use the same version 1 format and are
covered by a hand-written profile in the unit tests of
`crates/rupta-core/src/profile.rs`; the smoke test could not produce one
because the `excimer` extension is not installed on the test machine.

## Limits

- No symbolication of native frames and no source maps for JavaScript
  profile frames (minified browser frames stay minified).
- Measurements in chunks (CPU, memory) are stored in the document but not
  shown.
- Differential flamegraphs and function-level statistics are not
  available.
