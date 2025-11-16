import nodes from './nodes.js'
import tags from './tags.js'

// Extract version from workspace Cargo.toml
// This should be kept in sync with the workspace version
const ACTON_VERSION = '0.5.2'

// Helper function to build dependency string
function buildDep(features) {
  return `acton-service = { version = "${ACTON_VERSION}", features = [${features.map(f => `"${f}"`).join(', ')}] }`
}

const config = {
  nodes,
  tags,
  functions: {
    // Markdoc function to build cargo dependency with current version
    dep: {
      transform(parameters) {
        const features = parameters[0] || []
        if (typeof features === 'string') {
          return `acton-service = { version = "${ACTON_VERSION}", features = ["${features}"] }`
        }
        return buildDep(features)
      }
    }
  },
  variables: {
    version: {
      acton: ACTON_VERSION,
    },
    dep: {
      http: `acton-service = { version = "${ACTON_VERSION}", features = ["http", "observability"] }`,
      database: `acton-service = { version = "${ACTON_VERSION}", features = ["http", "observability", "database"] }`,
      cache: `acton-service = { version = "${ACTON_VERSION}", features = ["cache", "http", "observability"] }`,
      events: `acton-service = { version = "${ACTON_VERSION}", features = ["events", "http", "observability"] }`,
      grpc: `acton-service = { version = "${ACTON_VERSION}", features = ["grpc"] }`,
      openapi: `acton-service = { version = "${ACTON_VERSION}", features = ["openapi", "http", "observability"] }`,
      metrics: `acton-service = { version = "${ACTON_VERSION}", features = ["otel-metrics"] }`,
      full: `acton-service = { version = "${ACTON_VERSION}", features = ["full"] }`,
      httpOnly: `acton-service = { version = "${ACTON_VERSION}", features = ["http"] }`,
      observability: `acton-service = { version = "${ACTON_VERSION}", features = ["observability"] }`,
      grpcOnly: `acton-service = { version = "${ACTON_VERSION}", features = ["grpc"] }`,
      databaseOnly: `acton-service = { version = "${ACTON_VERSION}", features = ["database"] }`,
      cacheOnly: `acton-service = { version = "${ACTON_VERSION}", features = ["cache"] }`,
      eventsOnly: `acton-service = { version = "${ACTON_VERSION}", features = ["events"] }`,
      cedarAuthz: `acton-service = { version = "${ACTON_VERSION}", features = ["cedar-authz", "cache"] }`,
      resilience: `acton-service = { version = "${ACTON_VERSION}", features = ["resilience"] }`,
      governor: `acton-service = { version = "${ACTON_VERSION}", features = ["governor"] }`,
      otelMetrics: `acton-service = { version = "${ACTON_VERSION}", features = ["otel-metrics"] }`,
      openapiOnly: `acton-service = { version = "${ACTON_VERSION}", features = ["openapi"] }`,
      databaseCache: `acton-service = { version = "${ACTON_VERSION}", features = ["database", "cache"] }`,
    },
  },
}

export default config
