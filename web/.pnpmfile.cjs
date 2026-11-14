// typescript-eslint needs the JavaScript compiler API of TypeScript 6; the
// TypeScript 7 native compiler does not provide it. The project type-checks
// with TypeScript 7 (`pnpm typecheck`), and the linter packages get their
// own TypeScript 6 instead of the project's.
const TS6 = 'npm:typescript@6.0.3'

function readPackage(pkg) {
  const usesTsApi = pkg.name === 'typescript-eslint' || pkg.name === 'ts-api-utils' || (pkg.name ?? '').startsWith('@typescript-eslint/')
  if (usesTsApi && pkg.peerDependencies && pkg.peerDependencies.typescript) {
    delete pkg.peerDependencies.typescript
    pkg.dependencies = { ...pkg.dependencies, typescript: TS6 }
  }
  return pkg
}

module.exports = { hooks: { readPackage } }
