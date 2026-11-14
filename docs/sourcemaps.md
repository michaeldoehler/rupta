# Source maps: symbolication

JavaScript stack traces from minified bundles are mapped back to the
original source when the source maps were uploaded as artifact bundles
(see [source map upload](sourcemap-upload.md) for uploading with
`sentry-cli`). This page describes what happens at ingestion.

## Which frames

Frames of events with platform `javascript` or `node` whose `abs_path` (or
`filename`) ends in `.js`, `.mjs`, `.cjs` or `.jsx` and that have a line
and a column. Organizations without any artifact bundle are skipped
without a database query (the count is cached for 30 seconds).

## Finding the map

1. **Debug id.** The event's `debug_meta.images` entries of type
   `sourcemap` map a file (`code_file`, the frame's `abs_path`) to a
   `debug_id`, as written by `sentry-cli sourcemaps inject` and the bundler
   plugins. rupta looks up the `source_map` file with that debug id in
   `artifact_index` (newest bundle first). The minified file with the same
   debug id in the same bundle is used for function names.
2. **Release, dist and URL.** Without a debug id match, the event's
   `release` (and `dist`; no dist matches only files without one) and the
   frame's URL: `https://cdn.example.com/static/js/app.js?v=1` is tried as
   sent, without query, and as `~/static/js/app.js`, `~/js/app.js`,
   `~/app.js` (most specific first). A matching `minified_source` names its
   map in the manifest header `sourcemap` (resolved relative to the file's
   URL), or else `<url>.map` is used; a `source_map` file at the URL itself
   is used directly.

The bundle is read from the object store (`artifact_bundles.object_key`),
its `manifest.json` parsed, and the map decoded with the `sourcemap`
crate. Index maps (sections) are flattened.

## What changes in a frame

- `abs_path`: the original source as named in the map
  (`webpack://app/./src/cart.js`); `filename`: a readable form
  (`src/cart.js`).
- `lineno`, `colno`: the original position.
- `function`: the original name of the enclosing function, from the
  minified source and the map's names; when that fails, the name of the
  identifier called at the next older frame's position; else the minified
  name.
- `context_line`, `pre_context`, `post_context`: 5 lines before and after,
  from the map's `sourcesContent`. Maps without sources content leave the
  context empty.
- `in_app`: false when the original source is under `node_modules/`.
- `symbolicated: true`, and `raw` holds the frame as the SDK sent it.

Grouping and the issue title and culprit use the symbolicated frames.

When the event names a debug id for a file but no map with that id is
found, the event gets an entry `js_missing_source_map` in `errors` naming
the file. Frames without a map are left as they are.

## Caching

- Parsed maps and bundles: in memory, least recently used out, bounded by
  size (256 MiB together by default; a map costs about twice its JSON
  size).
- Lookups: the result of finding a map for a file (including "none") is
  kept for a minute, so a bundle uploaded after an event shows up for new
  events within a minute.

## Limits

- Only JavaScript source maps. No native symbols (minidumps, dSYM, PDB),
  no ProGuard or other mappings, no Hermes maps.
- Events are symbolicated once, at ingestion: uploading maps later does
  not change stored events.
- Function names in minified code depend on the map having `names`;
  maps without them keep minified names.
- No fetching of source maps from the internet: only uploaded bundles are
  used.
