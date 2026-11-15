import type { IntegrationKind } from '@/api/integrations'
import { BrandIcon } from '@/components/brand/brand-icon'
import type { TileSize } from '@/components/brand/brand-tile'

const DOCS = 'https://github.com/michaeldoehler/rupta/blob/main/docs/integrations.md'

export interface KindField {
  key: 'base_url' | 'email' | 'resolve_transition' | 'reopen_transition' | 'resolve_state' | 'reopen_state'
  label: string
  placeholder?: string
  required?: boolean
  hint?: string
}

export interface KindMeta {
  kind: IntegrationKind
  label: string
  short: string
  /** One line for the tracker picker. */
  description: string
  fields: KindField[]
  tokenLabel: string
  tokenHint: string
  /** Where the tracker shows tickets: "Jira project", "Linear team", … */
  targetLabel: string
  referenceExample: string
  webhookSteps: string
  /** Linear generates the signing secret itself. */
  trackerGeneratesSecret?: boolean
  docs: string
}

export const KINDS: KindMeta[] = [
  {
    kind: 'jira_cloud',
    label: 'Jira Cloud',
    short: 'Jira',
    description: 'Atlassian’s hosted Jira on atlassian.net.',
    fields: [
      { key: 'base_url', label: 'Site URL', placeholder: 'https://acme.atlassian.net', required: true },
      { key: 'email', label: 'Account email', placeholder: 'rupta-bot@acme.io', required: true, hint: 'The Atlassian account the API token belongs to.' },
      {
        key: 'resolve_transition',
        label: 'Resolve transition',
        placeholder: 'Done',
        hint: 'Default: the first transition into a Done-category status.',
      },
      {
        key: 'reopen_transition',
        label: 'Reopen transition',
        placeholder: 'Reopen',
        hint: 'Default: the first transition into a To Do-category status.',
      },
    ],
    tokenLabel: 'API token',
    tokenHint: 'id.atlassian.com → Security → API tokens → Create API token (a classic token, not a scoped one).',
    targetLabel: 'Jira project',
    referenceExample: 'OPS-42 or https://acme.atlassian.net/browse/OPS-42',
    webhookSteps: 'Jira settings → System → WebHooks → Create a WebHook: this URL, the secret, events Issue updated and Issue deleted.',
    docs: `${DOCS}#jira-cloud-jira_cloud`,
  },
  {
    kind: 'jira_server',
    label: 'Jira Data Center',
    short: 'Jira DC',
    description: 'Self-managed Jira Data Center or Server, 8.14 and later.',
    fields: [
      { key: 'base_url', label: 'Base URL', placeholder: 'https://jira.example.com', required: true },
      { key: 'resolve_transition', label: 'Resolve transition', placeholder: 'Done', hint: 'Optional, see the Jira Cloud defaults.' },
      { key: 'reopen_transition', label: 'Reopen transition', placeholder: 'Reopen' },
    ],
    tokenLabel: 'Personal access token',
    tokenHint: 'Profile → Personal Access Tokens → Create token (Jira 8.14 and later).',
    targetLabel: 'Jira project',
    referenceExample: 'OPS-42',
    webhookSteps:
      'Administration → System → WebHooks: this URL and events Issue updated and Issue deleted. Without a Secret field, append ?secret=<secret> to the URL.',
    docs: `${DOCS}#jira-data-center--server-jira_server`,
  },
  {
    kind: 'linear',
    label: 'Linear',
    short: 'Linear',
    description: 'Issues in a Linear team, with workflow states.',
    fields: [
      { key: 'resolve_state', label: 'Resolve state', placeholder: 'Done', hint: 'Default: the team’s first completed state.' },
      { key: 'reopen_state', label: 'Reopen state', placeholder: 'Todo', hint: 'Default: the first unstarted state.' },
    ],
    tokenLabel: 'API key',
    tokenHint: 'Linear → Settings → Security & access → Personal API keys (Read, Create issues, Create comments, Write).',
    targetLabel: 'Linear team',
    referenceExample: 'ENG-42 or https://linear.app/acme/issue/ENG-42',
    webhookSteps: 'Settings → API → Webhooks → New webhook: this URL, data change events Issues. Copy Linear’s signing secret into the field below.',
    trackerGeneratesSecret: true,
    docs: `${DOCS}#linear-linear`,
  },
  {
    kind: 'github',
    label: 'GitHub',
    short: 'GitHub',
    description: 'Issues in a repository, also GitHub Enterprise Server.',
    fields: [{ key: 'base_url', label: 'API URL', placeholder: 'https://api.github.com', hint: 'Only for GitHub Enterprise Server: https://<host>/api/v3.' }],
    tokenLabel: 'Personal access token',
    tokenHint: 'Fine-grained token with Issues: Read and write on the repositories (or a classic token with repo).',
    targetLabel: 'Repository',
    referenceExample: 'owner/repo#17, #17 or the issue URL',
    webhookSteps: 'Repository → Settings → Webhooks → Add webhook: this URL, content type application/json, the secret, individual event Issues.',
    docs: `${DOCS}#github-github`,
  },
  {
    kind: 'gitlab',
    label: 'GitLab',
    short: 'GitLab',
    description: 'Issues in a project on gitlab.com or self-managed.',
    fields: [{ key: 'base_url', label: 'GitLab URL', placeholder: 'https://gitlab.com', hint: 'Self-managed: https://gitlab.example.com.' }],
    tokenLabel: 'Access token (api scope)',
    tokenHint: 'Project or group access token (Reporter or higher), or a personal access token, with the api scope.',
    targetLabel: 'Project',
    referenceExample: 'group/project#5, #5 or the issue URL',
    webhookSteps: 'Project → Settings → Webhooks → Add new webhook: this URL, the secret as Secret token, trigger Issues events.',
    docs: `${DOCS}#gitlab-gitlab`,
  },
]

export function kindMeta(kind: IntegrationKind): KindMeta {
  return KINDS.find((k) => k.kind === kind) ?? KINDS[0]
}

/** The tracker's mark on a tile (see components/brand). */
export function TrackerIcon({
  kind,
  size = 'md',
  label,
  decorative,
  className,
}: {
  kind: IntegrationKind
  size?: TileSize
  label?: string
  decorative?: boolean
  className?: string
}) {
  return <BrandIcon kind={kind} size={size} label={label} decorative={decorative} className={className} />
}
