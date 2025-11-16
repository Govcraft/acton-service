/**
 * Centralized configuration for site-wide constants
 * This ensures DRY principle by having a single source of truth
 */

export const siteConfig = {
  /**
   * GitHub repository URL
   * Used in: header links, hero CTA buttons, documentation links
   */
  repositoryUrl: 'https://github.com/govcraft/acton-service',

  /**
   * Repository name (derived from URL)
   */
  repositoryName: 'govcraft/acton-service',
} as const
