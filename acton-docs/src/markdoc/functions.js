// Markdoc functions for version management
const ACTON_VERSION = '0.6.0'

export const version = {
  transform() {
    return ACTON_VERSION
  },
}

export const dep = {
  transform(parameters) {
    const [type] = Object.values(parameters)

    const dependencies = {
      // Common combinations
      http: `acton-service = { version = "${ACTON_VERSION}", features = ["http", "observability"] }`,
      database: `acton-service = { version = "${ACTON_VERSION}", features = ["http", "observability", "database"] }`,
      cache: `acton-service = { version = "${ACTON_VERSION}", features = ["cache", "http", "observability"] }`,
      events: `acton-service = { version = "${ACTON_VERSION}", features = ["events", "http", "observability"] }`,
      grpc: `acton-service = { version = "${ACTON_VERSION}", features = ["grpc"] }`,
      openapi: `acton-service = { version = "${ACTON_VERSION}", features = ["openapi", "http", "observability"] }`,
      metrics: `acton-service = { version = "${ACTON_VERSION}", features = ["otel-metrics"] }`,
      full: `acton-service = { version = "${ACTON_VERSION}", features = ["full"] }`,

      // Individual features
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

      // Multi-feature combinations
      databaseCache: `acton-service = { version = "${ACTON_VERSION}", features = ["database", "cache"] }`,
    }

    return dependencies[type] || `acton-service = { version = "${ACTON_VERSION}" }`
  },
}
