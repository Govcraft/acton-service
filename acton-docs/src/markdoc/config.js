import nodes from './nodes.js'
import tags from './tags.js'
import { siteConfig } from '../lib/config'

// Extract version from workspace Cargo.toml
// This should be kept in sync with the workspace version
const ACTON_VERSION = '0.30.0'

// Single source of truth mapping camelCase aliases to the real Cargo feature
// lists. Used by both the `dep()` function and the `$dep.*` variables so a
// `{% dep("grpcOnly") %}` call can never emit a feature name that does not
// exist in acton-service/Cargo.toml.
const DEP_FEATURES = {
  http: ['http', 'observability'],
  database: ['http', 'observability', 'database'],
  cache: ['cache', 'http', 'observability'],
  events: ['events', 'http', 'observability'],
  grpc: ['grpc'],
  openapi: ['openapi', 'http', 'observability'],
  metrics: ['otel-metrics'],
  full: ['full'],
  httpOnly: ['http'],
  observability: ['observability'],
  grpcOnly: ['grpc'],
  databaseOnly: ['database'],
  cacheOnly: ['cache'],
  eventsOnly: ['events'],
  cedarAuthz: ['cedar-authz', 'cache'],
  resilience: ['resilience'],
  governor: ['governor'],
  otelMetrics: ['otel-metrics'],
  prometheusMetrics: ['prometheus-metrics'],
  openapiOnly: ['openapi'],
  databaseCache: ['database', 'cache'],
  jwtOnly: ['jwt'],
  websocketOnly: ['websocket'],
  tursoOnly: ['turso'],
  surrealdbOnly: ['surrealdb'],
  clickhouse: ['clickhouse', 'http', 'observability'],
  clickhouseOnly: ['clickhouse'],
  clickhouseDatabase: ['clickhouse', 'database', 'http', 'observability'],
  clickhouseAudit: ['clickhouse', 'audit', 'http', 'observability'],
  audit: ['audit', 'http', 'observability'],
  auditOnly: ['audit'],
  auditDatabase: ['audit', 'database', 'http', 'observability'],
  loginLockout: ['login-lockout', 'http', 'observability'],
  journald: ['journald', 'http', 'observability'],
  journaldOnly: ['journald'],
}

// Helper function to build dependency string
function buildDep(features) {
  return `acton-service = { version = "${ACTON_VERSION}", features = [${features.map(f => `"${f}"`).join(', ')}] }`
}

const config = {
  nodes,
  tags,
  functions: {
    // Markdoc function to build cargo dependency with current version.
    // Accepts an alias from DEP_FEATURES, a literal feature name, or an
    // array of literal feature names.
    dep: {
      transform(parameters) {
        const arg = parameters[0] || []
        if (typeof arg === 'string') {
          return buildDep(DEP_FEATURES[arg] || [arg])
        }
        return buildDep(arg)
      }
    },
    // Current acton-service version, e.g. {% version() %}
    version: {
      transform() {
        return ACTON_VERSION
      }
    },
    // Function to build GitHub URLs
    githubUrl: {
      transform(parameters) {
        const path = parameters[0] || ''
        return siteConfig.repositoryUrl + path
      }
    }
  },
  variables: {
    version: {
      acton: ACTON_VERSION,
    },
    github: {
      repositoryUrl: siteConfig.repositoryUrl,
      repositoryName: siteConfig.repositoryName,
    },
    dep: Object.fromEntries(
      Object.entries(DEP_FEATURES).map(([alias, features]) => [alias, buildDep(features)])
    ),
  },
}

export default config
