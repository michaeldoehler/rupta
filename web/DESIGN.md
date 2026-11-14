# rupta design system: Lime Haze

One dark theme. The tokens live in `src/index.css` (`:root`, `color-scheme:
dark`); components use them through Tailwind (`bg-card`, `text-muted-foreground`,
`bg-brand`, …), never raw colours, so a change of tone happens in one place.

## Palette

| Name | Hex | Role |
| --- | --- | --- |
| Olive Charcoal | `#404436` | Strong borders, dividers; every neutral is derived from it and keeps its olive tint |
| Palm Leaf | `#779148` | Secondary accent: secondary buttons, "in app" and similar badges, distribution bars, chart secondary series, subtle tints |
| Electric Lime | `#CAF534` | Primary accent, used sparingly: primary buttons, focus rings, the active navigation indicator, the focused row's left marker, key highlights (issue volume chart), the logo |

The vertical gradient olive charcoal → palm leaf → electric lime
(`--brand-gradient`) appears only on the sign-in, setup and invitation pages.

## Tokens

| Token | Value | Use |
| --- | --- | --- |
| `--background` | `#15170f` | App background |
| `--sidebar` | `#181a12` | Top bar |
| `--card` | `#1c1f16` | Raised surfaces: panels, tables, cards |
| `--muted` | `#22251b` | Quiet fills (skeletons, bar tracks) |
| `--popover` | `#23271c` | Menus, dialogs, tooltips |
| `--secondary` | `#262a1f` | Neutral buttons |
| `--accent` | `#2f3326` | Hover |
| `--row-hover` / `--row-selected` | `#1f2218` / `#283020` | Table rows |
| `--border` | `#2f3326` | Dividers |
| `--border-strong` | `#404436` | Outline buttons, emphasised edges |
| `--input` | `#646b55` | Form control borders (3:1 against the surfaces) |
| `--foreground` | `#eef2e3` | Text |
| `--muted-foreground` | `#a9b09a` | Secondary text |
| `--subtle-foreground` | `#868d79` | Labels, captions, section titles |
| `--primary`, `--brand` | `#caf534` | Lime accent; text on it is `--primary-foreground` `#171a0d` |
| `--brand-soft` / `--brand-text` | `#2a3313` / `#c3dc86` | Lime tint and lime-leaning text (links, active labels) |
| `--palm` / `--palm-soft` / `--palm-text` | `#779148` / `#26301a` / `#a5c172` | Palm accent, its tint and readable palm text |
| `--destructive`, `--level-error` | `#ee6a58` | Errors, destructive actions |
| `--level-fatal` | `#e65386` | Fatal |
| `--warning`, `--level-warning` | `#e9b44c` | Warnings |
| `--level-info` | `#6aa6e6` | Info |
| `--success` | `#4cc59a` | Resolved, healthy (teal green, kept apart from lime) |
| `--level-debug` | `#8b927e` | Debug |
| `--code-bg` / `--code-line-hl` | `#12140d` / `#262f14` | Code blocks, highlighted line |
| `--series-1 … 5` | `#caf534 #5497d9 #cb7d34 #a074d0 #37a69a` | Categorical chart series, fixed order |
| `--series-other` | `#868d79` | Uncategorised marks |

Syntax highlighting uses the "lime haze" theme in `src/features/event/highlight.ts`:
keywords lime, strings palm, numbers and parameters coral, functions sand,
types sage, comments subtle.

## Rules

- Lime is a signal. Use it for one primary action per view, focus, the
  current location (navigation underline, focused row marker) and the one
  number or chart that matters most. Never fill large areas with it
  outside the sign-in pages.
- Links are `text-brand-text` or `text-palm-text` and turn lime on hover.
- Secondary actions use the outline (olive border) or secondary (palm tint)
  button variants.
- Semantic colours stay semantic: red for errors, raspberry for fatal,
  amber for warnings, blue for info, teal green for success. They never
  stand for "series 4"; a chart that needs more than five series lists the
  rest in a table.
- Status is never colour alone: badges carry a word, markers a tooltip or
  label.
- Charts: recessive grid and axes, one y-axis, a legend for two or more
  series. The categorical palette was checked with a colour-vision
  simulation (adjacent pairs ΔE ≥ 10 for deuteranopia, protanopia and
  tritanopia, all ≥ 3:1 against the card surface). Lime is brighter than
  the recommended lightness band for dark charts on purpose; it is always
  the first series and never next to a similar hue.

## Contrast (WCAG 2.2)

Body text needs 4.5:1, large text and UI parts 3:1.

| Foreground | Background | Ratio |
| --- | --- | --- |
| foreground `#eef2e3` | background `#15170f` | 15.9 |
| foreground `#eef2e3` | card `#1c1f16` | 14.7 |
| foreground `#eef2e3` | row-selected `#283020` | 12.0 |
| muted-foreground `#a9b09a` | background `#15170f` | 8.1 |
| muted-foreground `#a9b09a` | card `#1c1f16` | 7.4 |
| muted-foreground `#a9b09a` | popover `#23271c` | 6.8 |
| subtle-foreground `#868d79` | background `#15170f` | 5.3 |
| subtle-foreground `#868d79` | card `#1c1f16` | 4.9 |
| primary-foreground `#171a0d` | lime `#caf534` | 14.0 |
| brand-text `#c3dc86` | background `#15170f` | 12.0 |
| brand-text `#c3dc86` | brand-soft `#2a3313` | 8.8 |
| palm-text `#a5c172` | background `#15170f` | 9.0 |
| palm-text `#a5c172` | palm-soft `#26301a` | 6.9 |
| lime `#caf534` (focus ring, indicator) | background `#15170f` | 14.3 |
| palm `#779148` (bars, marks) | background `#15170f` | 5.1 |
| input border `#646b55` | background / card | 3.3 / 3.0 |
| destructive `#ee6a58` | card `#1c1f16` | 5.4 |
| level-fatal `#e65386` | card `#1c1f16` | 4.7 |
| warning `#e9b44c` | card `#1c1f16` | 8.8 |
| level-info `#6aa6e6` | card `#1c1f16` | 6.5 |
| success `#4cc59a` | card `#1c1f16` | 7.8 |
| level-debug `#8b927e` | card `#1c1f16` | 5.2 |
| code text `#dfe5d2` | code-bg `#12140d` | 14.4 |
| code comment `#7c8471` | code-bg `#12140d` | 4.8 |
| code keyword `#caf534` / string `#a5c172` / number `#f0a77c` | code-bg | 14.7 / 9.3 / 9.3 |
| code function `#e6d9a8` / type `#9fd0c4` | code-bg | 13.1 / 10.9 |
| bar label `#171a0d` | series blue `#5497d9` / grey `#868d79` | 5.7 / 5.1 |

`--border` (`#2f3326`) and `--border-strong` (`#404436`) separate areas
and are not the only cue for any control; form fields use `--input`.

## Service marks

Third-party services (issue trackers, notification channels, the ZITADEL
sign-in provider) are shown with their own marks, bundled as inline SVG in
`src/components/brand/` (sources and terms in its `NOTICE.md`; nothing is
loaded from vendor sites). They are the only place where colours outside the
tokens appear.

- Every mark sits on a rounded tile of 16, 20, 24, 32 or 40 px
  (`BrandIcon`, `SsoProviderIcon`): the brand-coloured glyph on a light tile
  (`--foreground`) for dark or low-contrast brand colours (Jira, GitHub,
  GitLab, Opsgenie, ZITADEL), a white glyph on the brand colour where that is
  the brand's usual form (Linear, PagerDuty). Brands whose terms do not allow
  their logo (Slack, Microsoft Teams) get a letter on the brand colour;
  webhook, email and other OIDC providers a lucide icon on a neutral tile.
- A tile names its service (`aria-label` and `title`); next to text that
  already names it, it is decorative.
- Pickers of services (`BrandPicker`) are radio groups of tiles: palm
  border, palm tint and a 1 px lift on hover (no lift with reduced motion),
  the lime focus ring, and a lime border on brand tint when selected. The
  arrow keys move the selection.

## Layout

- A sticky top bar (48 px): logo and organization on the left, the main
  navigation (Issues, Explore ▾ with Performance, Traces, Logs, Metrics,
  Dashboards, Profiles, Replays, then Releases, Crons, Feedback, Alerts),
  search (⌘K), settings and the account menu on the right. The current
  section has a lime underline. Below 1024 px the navigation moves into a
  menu.
- Page headers carry the project, environment and period selectors.
- Detail pages put the content in a main column and context in a sticky
  right panel (issue details: status and assignment, volume, first and
  last seen, trace and replay, top tags, the event's context,
  participants; trace view: span details; replays: breadcrumbs, console,
  network, errors). New context, such as linked tickets, is added as
  another panel section.
- On screens at least 1440 px wide the issue stream can show a preview of
  the focused issue next to the list (remembered per browser).
