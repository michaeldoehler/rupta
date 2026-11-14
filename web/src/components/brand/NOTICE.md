# Third-party marks

The names and logos of Jira, Linear, GitHub, GitLab, PagerDuty, Opsgenie,
Slack, Microsoft Teams and ZITADEL are trademarks of their respective
owners. rupta shows them only to identify the service an integration,
notification channel or sign-in provider connects to. Their use does not
imply any affiliation with or endorsement by the owners.

All marks are bundled with the web interface as inline SVG. Nothing is
loaded from the vendors' sites at runtime, so the interface works offline
and in air-gapped installations and needs no extra Content Security Policy
entries.

## Sources

| Service                              | Mark                                         | Source                                                                                                            | Terms                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------ | -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Jira (Cloud and Data Center)         | Jira logo, `#0052CC`                         | simple-icons 16.37.0, `icons/jira.svg`                                                                            | simple-icons, CC0-1.0; brand guidelines: https://atlassian.design/foundations/logos/                                                                                                                                                                                                                                           |
| Linear                               | Linear logo, `#5E6AD2`                       | simple-icons 16.37.0, `icons/linear.svg`                                                                          | simple-icons, CC0-1.0                                                                                                                                                                                                                                                                                                          |
| GitHub                               | Invertocat, `#181717`                        | simple-icons 16.37.0, `icons/github.svg`                                                                          | simple-icons, CC0-1.0; brand guidelines: https://github.com/logos                                                                                                                                                                                                                                                              |
| GitLab                               | Tanuki, `#FC6D26`                            | simple-icons 16.37.0, `icons/gitlab.svg`                                                                          | simple-icons, CC0-1.0; trademark guidelines linked from https://about.gitlab.com/press/press-kit/                                                                                                                                                                                                                              |
| PagerDuty                            | PagerDuty logo, `#06AC38`                    | simple-icons 16.37.0, `icons/pagerduty.svg`                                                                       | simple-icons, CC0-1.0; brand guidelines: https://www.pagerduty.com/brand/                                                                                                                                                                                                                                                      |
| Opsgenie                             | Opsgenie logo, `#172B4D`                     | simple-icons 16.37.0, `icons/opsgenie.svg`                                                                        | simple-icons, CC0-1.0; source: Atlassian press kit                                                                                                                                                                                                                                                                             |
| ZITADEL                              | "solo" mark for light backgrounds, unchanged | `console/src/assets/images/zitadel-logo-solo-dark.svg` in https://github.com/zitadel/zitadel (fetched 2026-11-14) | ZITADEL brand and trademark policy (https://github.com/zitadel/zitadel/blob/main/apps/docs/content/legal/policies/brand-trademark-policy.mdx): fair use includes "indicating that the software is available for use or installation without implying any affiliation or endorsement"; the official logo must be used unaltered |
| Slack                                | Monogram "S" on `#4A154B`, no logo           | none                                                                                                              | simple-icons 16.37.0 does not contain it, and the Slack Brand Terms of Service (https://slack.com/terms-of-service/slack-brand) give no general permission to show the logo in a third-party product                                                                                                                           |
| Microsoft Teams                      | Monogram "T" on `#5059C9`, no logo           | none                                                                                                              | simple-icons 16.37.0 does not contain it, and Microsoft's trademark guidelines (https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks) require an express license for its logos and product icons                                                                                                              |
| Webhook, email, other OIDC providers | Lucide icons (`Webhook`, `Mail`, `KeyRound`) | lucide-react                                                                                                      | ISC                                                                                                                                                                                                                                                                                                                            |

The simple-icons path data is copied unchanged into `marks.ts`; the
simple-icons package is not a dependency. simple-icons is released under
CC0-1.0, and its data (`data/simple-icons.json`) lists no other license for
these six icons. Its disclaimer asks users to follow each brand's
guidelines, which remain the owners' to set; rupta uses each mark only next to
the service's name to identify the integration and does not alter it apart
from colour on a tile (the brand colour, or white on a tile in the brand
colour).

To add a brand, copy its path and `hex` from the current simple-icons
release into `marks.ts`, add it to `BRANDS` in `brands.ts`, and add a row
here. Check the brand's guidelines first; if simple-icons has removed it,
use the vendor's official asset only when its terms allow this use, and a
monogram otherwise.
