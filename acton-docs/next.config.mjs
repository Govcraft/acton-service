import withMarkdoc from '@markdoc/next.js'

import withSearch from './src/markdoc/search.mjs'

/** @type {import('next').NextConfig} */
const nextConfig = {
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
