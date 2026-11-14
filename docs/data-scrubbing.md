# Data scrubbing

rupta removes sensitive values from every event before it is stored. The
SDKs can scrub on their side too (`send_default_pii` off, `beforeSend`
hooks); server-side scrubbing is a second line that also covers SDKs you
do not control.

Settings are per project (`settings.scrubbing`, editable in the project
settings or the API):

| Setting | Default | Effect |
| --- | --- | --- |
| `enabled` | `true` | Apply the rules below. |
| `extra_fields` | `[]` | More key names to scrub (case-insensitive substring match). |
| `safe_fields` | `[]` | Keys never scrubbed (exact, case-insensitive). |
| `scrub_ip_addresses` | `false` | Do not store IP addresses. |

## What is replaced

Values become `[Filtered]` when

- **the key** contains (case-insensitive) one of: `password`, `passwd`,
  `pwd`, `secret`, `token`, `auth`, `credential`, `cookie`, `session`,
  `csrf`, `xsrf`, `api_key`, `apikey`, `api-key`, `private_key`,
  `privatekey`, `private-key`, `access_key`, `accesskey`, `signature`,
  `ssn`, `credit_card`, `creditcard`, `card_number`, `cardnumber`, `cvv`,
  `cvc`, `iban`, or one of the project's `extra_fields`. A few harmless
  keys are exempt (`author`, `authority`, `oauth_provider`, `tokenizer`,
  `session_id_hash`), as are the project's `safe_fields`. Matching applies
  at any depth of nested data;
- **the value** contains a credit card number (13–19 digits, spaces or
  dashes allowed, passing the Luhn check), an `Authorization`-style
  credential (`Bearer …`, `Basic …`, `Token …`), a private key block, or a
  password in a URL (`postgres://user:secret@host` keeps the user).

Patterns replace only the matching part of a string; key matches replace
the whole value.

## Where

- Request: all `Authorization`, `Proxy-Authorization`, `Cookie` and
  `Set-Cookie` headers and all cookie values, always; other headers, the
  query string (also inside the URL), form and JSON bodies and the
  environment by the rules above.
- `extra`, all contexts, user data, frame variables.
- Breadcrumb messages and data.
- Event message and exception values (patterns only).
- Tags.

With `scrub_ip_addresses`, `user.ip_address`, the `X-Forwarded-For`,
`X-Real-IP`, `Forwarded` and `CF-Connecting-IP` headers, `REMOTE_ADDR` in
the request environment and the `user` tag derived from an IP address are
removed (this applies even with `enabled` off). The address is then also
missing from `events.user_ip`, so users identified only by IP are not
counted in `users_seen`.

## Limits

- Scrubbing works on key names and a few value patterns. It does not find
  personal data in free text (names, addresses, phone numbers) unless the
  key says so; add such keys to `extra_fields`.
- Values are replaced, not hashed: scrubbed values cannot be searched.
- Attachments are stored as sent; they are not scrubbed.
- Rules apply to events stored after a change; stored events are not
  rewritten.
- Transactions, sessions and check-ins are not scrubbed (they carry names,
  tags and durations, not request data).
