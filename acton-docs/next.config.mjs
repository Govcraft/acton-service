import withMarkdoc from '@markdoc/next.js'

import * as fs from 'fs'

import withSearch from './src/markdoc/search.mjs'

// The version the docs show is the crate's: read once from the workspace
// manifest so no page, snippet or the logo carries its own copy. A manifest
// without one fails the build rather than publishing an empty version.
function workspaceVersion() {
  const manifest = fs.readFileSync(new URL('../Cargo.toml', import.meta.url), 'utf8')
  const section = manifest.split(/^\[workspace\.package\]\s*$/m)[1]?.split(/^\[/m)[0] ?? ''
  const version = section.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
  if (!version) {
    throw new Error('no version under [workspace.package] in ../Cargo.toml')
  }
  return version
}

const ACTON_VERSION = workspaceVersion()

/** @type {import('next').NextConfig} */
const nextConfig = {
  env: { ACTON_VERSION },
  pageExtensions: ['js', 'jsx', 'md', 'ts', 'tsx'],
  output: 'export',
  images: {
    unoptimized: true,
  },
  // Base path for GitHub Pages (repo name)
  basePath: process.env.GITHUB_ACTIONS ? '/acton-service' : '',
  // Asset prefix for GitHub Pages
  assetPrefix: process.env.GITHUB_ACTIONS ? '/acton-service/' : '',
}

export default withSearch(
  withMarkdoc({ schemaPath: './src/markdoc' })(nextConfig),
)
