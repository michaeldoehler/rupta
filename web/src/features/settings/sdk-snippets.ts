// SDK setup instructions per platform. This is the only place where the
// official SDK package names appear (they are what users install).

import type { Lang } from '@/features/event/highlight'

export interface SnippetStep {
  title: string
  code: string
  lang: Lang
  file?: string
}

export interface PlatformGuide {
  id: string
  label: string
  /** Project platforms this guide is the default for. */
  platforms: string[]
  steps: (dsn: string, release: string) => SnippetStep[]
  note?: string
}

export const SDK_GUIDES: PlatformGuide[] = [
  {
    id: 'javascript',
    label: 'JavaScript (browser)',
    platforms: ['javascript'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'npm install @sentry/browser' },
      {
        title: 'Initialize it as early as possible',
        lang: 'javascript',
        file: 'src/main.js',
        code: `import * as Sentry from "@sentry/browser";\n\nSentry.init({\n  dsn: "${dsn}",\n  release: "${release}",\n  environment: "production",\n  sendDefaultPii: false,\n});`,
      },
    ],
  },
  {
    id: 'react',
    label: 'React',
    platforms: ['javascript-react'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'npm install @sentry/react' },
      {
        title: 'Initialize it before rendering the app',
        lang: 'tsx',
        file: 'src/main.tsx',
        code: `import * as Sentry from "@sentry/react";\nimport { createRoot } from "react-dom/client";\nimport { App } from "./App";\n\nSentry.init({\n  dsn: "${dsn}",\n  release: "${release}",\n  environment: "production",\n});\n\ncreateRoot(document.getElementById("root")!, {\n  onUncaughtError: Sentry.reactErrorHandler(),\n  onCaughtError: Sentry.reactErrorHandler(),\n}).render(<App />);`,
      },
    ],
  },
  {
    id: 'nextjs',
    label: 'Next.js',
    platforms: ['javascript-nextjs'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'npm install @sentry/nextjs' },
      {
        title: 'Client side',
        lang: 'typescript',
        file: 'instrumentation-client.ts',
        code: `import * as Sentry from "@sentry/nextjs";\n\nSentry.init({\n  dsn: "${dsn}",\n  release: "${release}",\n});`,
      },
      {
        title: 'Server side',
        lang: 'typescript',
        file: 'sentry.server.config.ts',
        code: `import * as Sentry from "@sentry/nextjs";\n\nSentry.init({\n  dsn: "${dsn}",\n  release: "${release}",\n});`,
      },
      {
        title: 'Load the server config',
        lang: 'typescript',
        file: 'instrumentation.ts',
        code: `export async function register() {\n  if (process.env.NEXT_RUNTIME === "nodejs") {\n    await import("./sentry.server.config");\n  }\n}\n\nexport { captureRequestError as onRequestError } from "@sentry/nextjs";`,
      },
    ],
  },
  {
    id: 'node',
    label: 'Node.js',
    platforms: ['node'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'npm install @sentry/node' },
      {
        title: 'Create instrument.js',
        lang: 'javascript',
        file: 'instrument.js',
        code: `const Sentry = require("@sentry/node");\n\nSentry.init({\n  dsn: "${dsn}",\n  release: "${release}",\n  environment: process.env.NODE_ENV,\n});`,
      },
      { title: 'Load it before anything else', lang: 'bash', code: 'node --require ./instrument.js app.js' },
    ],
  },
  {
    id: 'php',
    label: 'PHP',
    platforms: ['php'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'composer require sentry/sentry' },
      {
        title: 'Initialize it at the start of your app',
        lang: 'php',
        code: `\\Sentry\\init([\n    'dsn' => '${dsn}',\n    'release' => '${release}',\n    'environment' => 'production',\n]);`,
      },
    ],
  },
  {
    id: 'laravel',
    label: 'Laravel',
    platforms: ['php-laravel'],
    steps: (dsn) => [
      { title: 'Install the SDK', lang: 'bash', code: 'composer require sentry/sentry-laravel' },
      { title: 'Publish the config and set the DSN', lang: 'bash', code: `php artisan sentry:publish --dsn=${dsn}` },
      {
        title: 'Report exceptions',
        lang: 'php',
        file: 'bootstrap/app.php',
        code: `use Sentry\\Laravel\\Integration;\n\n->withExceptions(function (Exceptions $exceptions) {\n    Integration::handles($exceptions);\n})`,
      },
      { title: 'Or set it in .env', lang: 'bash', file: '.env', code: `SENTRY_LARAVEL_DSN=${dsn}` },
    ],
  },
  {
    id: 'symfony',
    label: 'Symfony',
    platforms: ['php-symfony'],
    steps: (dsn) => [
      { title: 'Install the bundle', lang: 'bash', code: 'composer require sentry/sentry-symfony' },
      { title: 'Set the DSN', lang: 'bash', file: '.env', code: `SENTRY_DSN="${dsn}"` },
      {
        title: 'Configure (optional)',
        lang: 'yaml',
        file: 'config/packages/sentry.yaml',
        code: `when@prod:\n  sentry:\n    dsn: '%env(SENTRY_DSN)%'\n    options:\n      environment: '%kernel.environment%'`,
      },
    ],
  },
  {
    id: 'python',
    label: 'Python',
    platforms: ['python', 'python-flask', 'python-fastapi'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'pip install --upgrade sentry-sdk' },
      {
        title: 'Initialize it at startup',
        lang: 'python',
        code: `import sentry_sdk\n\nsentry_sdk.init(\n    dsn="${dsn}",\n    release="${release}",\n    environment="production",\n    send_default_pii=False,\n)`,
      },
    ],
    note: 'Integrations for Flask, FastAPI, Celery and others are enabled automatically when the framework is installed.',
  },
  {
    id: 'django',
    label: 'Django',
    platforms: ['python-django'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'pip install --upgrade "sentry-sdk[django]"' },
      {
        title: 'Initialize it in settings.py',
        lang: 'python',
        file: 'settings.py',
        code: `import sentry_sdk\n\nsentry_sdk.init(\n    dsn="${dsn}",\n    release="${release}",\n    environment="production",\n)`,
      },
    ],
  },
  {
    id: 'go',
    label: 'Go',
    platforms: ['go'],
    steps: (dsn, release) => [
      { title: 'Install the SDK', lang: 'bash', code: 'go get github.com/getsentry/sentry-go' },
      {
        title: 'Initialize it in main',
        lang: 'go',
        file: 'main.go',
        code: `err := sentry.Init(sentry.ClientOptions{\n\tDsn:         "${dsn}",\n\tRelease:     "${release}",\n\tEnvironment: "production",\n})\nif err != nil {\n\tlog.Fatalf("sentry.Init: %s", err)\n}\n// Send buffered events before the program exits.\ndefer sentry.Flush(2 * time.Second)`,
      },
    ],
  },
  {
    id: 'java',
    label: 'Java',
    platforms: ['java'],
    steps: (dsn, release) => [
      { title: 'Add the dependency (Gradle)', lang: 'bash', code: `implementation("io.sentry:sentry:<latest version>")` },
      {
        title: 'Initialize it at startup',
        lang: 'java',
        code: `import io.sentry.Sentry;\n\nSentry.init(options -> {\n    options.setDsn("${dsn}");\n    options.setRelease("${release}");\n    options.setEnvironment("production");\n});`,
      },
    ],
    note: 'Spring Boot users can add io.sentry:sentry-spring-boot-starter-jakarta and set sentry.dsn in application.properties.',
  },
  {
    id: 'dotnet',
    label: '.NET',
    platforms: ['dotnet'],
    steps: (dsn, release) => [
      { title: 'Install the package', lang: 'bash', code: 'dotnet add package Sentry' },
      {
        title: 'Initialize it at startup',
        lang: 'csharp',
        code: `using Sentry;\n\nSentrySdk.Init(options =>\n{\n    options.Dsn = "${dsn}";\n    options.Release = "${release}";\n    options.Environment = "production";\n});`,
      },
    ],
    note: 'For ASP.NET Core, install Sentry.AspNetCore and call builder.WebHost.UseSentry(o => o.Dsn = "…").',
  },
  {
    id: 'ruby',
    label: 'Ruby',
    platforms: ['ruby'],
    steps: (dsn, release) => [
      { title: 'Add the gem', lang: 'ruby', file: 'Gemfile', code: `gem "sentry-ruby"\n# for Rails:\ngem "sentry-rails"` },
      {
        title: 'Initialize it',
        lang: 'ruby',
        file: 'config/initializers/sentry.rb',
        code: `Sentry.init do |config|\n  config.dsn = "${dsn}"\n  config.release = "${release}"\n  config.environment = "production"\nend`,
      },
    ],
  },
  {
    id: 'rust',
    label: 'Rust',
    platforms: ['rust'],
    steps: (dsn) => [
      { title: 'Add the crate', lang: 'bash', code: 'cargo add sentry' },
      {
        title: 'Initialize it first thing in main',
        lang: 'rust',
        file: 'src/main.rs',
        code: `fn main() {\n    let _guard = sentry::init((\n        "${dsn}",\n        sentry::ClientOptions {\n            release: sentry::release_name!(),\n            environment: Some("production".into()),\n            ..Default::default()\n        },\n    ));\n    // Keep _guard alive for the whole program.\n}`,
      },
    ],
  },
]

export function guideFor(platform: string | null | undefined): PlatformGuide {
  return (
    SDK_GUIDES.find((g) => platform && g.platforms.includes(platform)) ??
    SDK_GUIDES.find((g) => platform && g.platforms.some((p) => platform.startsWith(p.split('-')[0]))) ??
    SDK_GUIDES[0]
  )
}
