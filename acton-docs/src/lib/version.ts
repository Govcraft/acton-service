/**
 * The acton-service version every page, snippet and the logo show.
 *
 * Not a literal: `next.config.mjs` reads it from `[workspace.package]` in the
 * repository's root `Cargo.toml` at build time, so a release that bumps the
 * crate bumps the docs with it and the two cannot drift.
 */
export const VERSION: string = process.env.ACTON_VERSION ?? ''
