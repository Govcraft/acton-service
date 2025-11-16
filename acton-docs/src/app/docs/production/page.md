---
title: Production Checklist
nextjs:
  metadata:
    title: Production Checklist
    description: Comprehensive pre-deployment checklist for acton-service applications covering security, performance, and reliability.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Ensure your acton-service application is production-ready with this comprehensive checklist covering security, observability, performance, and operational excellence.

## Configuration Review

### Service Configuration

- [ ] Set explicit service name in configuration
- [ ] Configure appropriate port binding (default: 8080)
- [ ] Enable TLS/HTTPS for production traffic
- [ ] Set connection timeout values appropriate for workload
- [ ] Configure graceful shutdown timeout
- [ ] Review and set request body size limits
- [ ] Enable compression for responses (gzip, br, deflate, zstd)

### Environment Variables

- [ ] Use environment-specific configuration files
- [ ] Validate all required environment variables are set
- [ ] Store sensitive values in secure secret management
- [ ] Never commit secrets to version control
- [ ] Use XDG-compliant config directories (`~/.config/acton-service/`)
- [ ] Document all required configuration values

### Feature Flags

- [ ] Enable only required features to minimize dependencies
- [ ] Review enabled middleware and their performance impact
- [ ] Disable development-only features (verbose logging, debug endpoints)
- [ ] Configure observability features (`observability` flag)

## Security Hardening

### Authentication & Authorization

- [ ] Implement JWT authentication for protected endpoints
- [ ] Use strong signing algorithms (RS256, ES256 preferred over HS256)
- [ ] Rotate JWT signing keys regularly
- [ ] Set appropriate token expiration times
- [ ] Implement token revocation with Redis backend
- [ ] Configure Cedar policy-based authorization for fine-grained access
- [ ] Review and test all Cedar policies before deployment
- [ ] Validate JWT claims structure (roles, permissions, user/client ID)

### Network Security

- [ ] Enable CORS with restrictive origins (not `*` in production)
- [ ] Configure TLS with modern cipher suites
- [ ] Use HTTPS-only in production
- [ ] Implement rate limiting (per-user and per-client)
- [ ] Set up distributed rate limiting with Redis for multi-instance deployments
- [ ] Configure firewall rules to restrict service access

### Input Validation

- [ ] Set maximum request body sizes
- [ ] Validate all user inputs
- [ ] Implement request timeouts to prevent slow clients
- [ ] Enable panic recovery middleware
- [ ] Configure sensitive header masking in logs

### Secrets Management

- [ ] Never hardcode credentials in code or config files
- [ ] Use environment variables or secret managers (Vault, AWS Secrets Manager)
- [ ] Rotate database credentials regularly
- [ ] Secure Redis connection strings
- [ ] Protect NATS authentication tokens
- [ ] Review file permissions on configuration files

## Health Check Validation

### Kubernetes Probes

- [ ] Test `/health` endpoint returns 200 when service is healthy
- [ ] Test `/ready` endpoint validates all dependencies
- [ ] Configure appropriate `initialDelaySeconds` for startup time
- [ ] Set `periodSeconds` to balance responsiveness and overhead
- [ ] Configure `failureThreshold` to prevent flapping
- [ ] Test liveness probe triggers pod restart on failure
- [ ] Verify readiness probe removes unhealthy pods from load balancer

### Dependency Health

- [ ] Verify database connectivity in readiness check
- [ ] Verify Redis connectivity in readiness check
- [ ] Verify NATS connectivity in readiness check
- [ ] Configure appropriate health check timeouts
- [ ] Test service behavior when dependencies are unavailable

## Observability Setup

### Logging

- [ ] Enable structured JSON logging for production
- [ ] Set appropriate log level (`info` or `warn` for production)
- [ ] Configure log aggregation (ELK, Loki, CloudWatch)
- [ ] Verify sensitive data is masked in logs
- [ ] Enable request ID generation and propagation
- [ ] Configure correlation IDs for distributed tracing

### Metrics

- [ ] Enable OpenTelemetry metrics collection
- [ ] Configure Prometheus scraping endpoint (`/metrics`)
- [ ] Set up ServiceMonitor for Kubernetes deployments
- [ ] Monitor HTTP request count and duration
- [ ] Monitor active request count
- [ ] Monitor request/response sizes
- [ ] Set up alerting on key metrics (error rate, latency, throughput)

### Distributed Tracing

- [ ] Enable OpenTelemetry tracing
- [ ] Configure trace exporter (Jaeger, Zipkin, OTLP)
- [ ] Propagate trace headers (x-request-id, x-trace-id, x-span-id)
- [ ] Sample traces appropriately for production load
- [ ] Verify trace context propagation across services

### Alerting

- [ ] Set up alerts for high error rates (>1%)
- [ ] Set up alerts for high latency (p99 > threshold)
- [ ] Set up alerts for pod restart loops
- [ ] Set up alerts for resource exhaustion
- [ ] Set up alerts for circuit breaker trips
- [ ] Configure PagerDuty/Opsgenie for critical alerts

## Performance Optimization

### Resource Configuration

- [ ] Set appropriate memory requests and limits
- [ ] Set appropriate CPU requests and limits
- [ ] Configure connection pool sizes for database
- [ ] Configure connection pool sizes for Redis
- [ ] Configure NATS connection pool settings
- [ ] Tune request timeout values

### Resilience Patterns

- [ ] Enable circuit breaker middleware
- [ ] Configure failure rate threshold (recommended: 0.5 = 50%)
- [ ] Enable retry logic with exponential backoff
- [ ] Set maximum retry attempts (recommended: 3)
- [ ] Enable bulkhead pattern for concurrency limiting
- [ ] Set appropriate concurrent request limits
- [ ] Test circuit breaker behavior under failure

### Caching

- [ ] Configure Redis caching for Cedar policy decisions
- [ ] Set appropriate cache TTLs
- [ ] Implement cache warming for critical data
- [ ] Monitor cache hit rates
- [ ] Configure cache eviction policies

### Database Optimization

- [ ] Enable connection pooling with appropriate size
- [ ] Set connection timeout and idle timeout
- [ ] Create database indexes for frequent queries
- [ ] Test query performance under load
- [ ] Enable prepared statement caching

## Deployment Strategy

### Container Configuration

- [ ] Use multi-stage Docker builds
- [ ] Run containers as non-root user
- [ ] Use minimal base images (debian:bookworm-slim)
- [ ] Install only required dependencies
- [ ] Add Docker health checks
- [ ] Scan images for vulnerabilities (Docker Scout, Trivy)
- [ ] Use specific version tags (not `latest`)

### Kubernetes Configuration

- [ ] Deploy with 3+ replicas for high availability
- [ ] Configure rolling update strategy
- [ ] Set `maxSurge: 1` and `maxUnavailable: 0` for zero-downtime
- [ ] Enable horizontal pod autoscaling (HPA)
- [ ] Configure pod disruption budget (PDB)
- [ ] Set resource requests and limits
- [ ] Use dedicated namespaces per environment

### API Versioning

- [ ] All routes use `VersionedApiBuilder`
- [ ] Version deprecated APIs with deprecation warnings
- [ ] Document API version lifecycle
- [ ] Plan for version sunset timeline
- [ ] Test backward compatibility
- [ ] Provide migration guides for version changes

## Pre-Deployment Testing

### Functional Testing

- [ ] All unit tests passing
- [ ] All integration tests passing
- [ ] End-to-end tests covering critical paths
- [ ] Test all API versions
- [ ] Test health and readiness endpoints
- [ ] Test graceful shutdown behavior

### Load Testing

- [ ] Perform load testing at expected production traffic levels
- [ ] Test at 2x expected peak traffic
- [ ] Verify latency under load (p50, p95, p99)
- [ ] Test circuit breaker activation under failure
- [ ] Test rate limiting behavior
- [ ] Monitor resource usage under load

### Security Testing

- [ ] Run security scanner on dependencies (`cargo audit`)
- [ ] Test JWT authentication with invalid tokens
- [ ] Test authorization with unauthorized users
- [ ] Test CORS configuration
- [ ] Test rate limiting enforcement
- [ ] Validate input sanitization

## Monitoring & Maintenance

### Operational Procedures

- [ ] Document runbook for common issues
- [ ] Document rollback procedures
- [ ] Set up log aggregation and search
- [ ] Configure automated backups for databases
- [ ] Test disaster recovery procedures
- [ ] Document on-call escalation process

### Continuous Monitoring

- [ ] Monitor error rates and latency trends
- [ ] Monitor resource utilization (CPU, memory, disk)
- [ ] Monitor database connection pool usage
- [ ] Monitor cache hit rates
- [ ] Track API version usage
- [ ] Monitor circuit breaker metrics

### Regular Maintenance

- [ ] Update dependencies regularly (`cargo update`)
- [ ] Scan for security vulnerabilities regularly
- [ ] Review and rotate credentials quarterly
- [ ] Review and update Cedar policies as needed
- [ ] Monitor and archive old logs
- [ ] Review and optimize database queries

## Go-Live Checklist

Final checks before deploying to production:

- [ ] All above sections reviewed and completed
- [ ] Configuration reviewed and approved
- [ ] Security audit completed
- [ ] Load testing passed
- [ ] Monitoring and alerting configured
- [ ] Runbook documented
- [ ] Rollback plan prepared
- [ ] Team trained on operational procedures
- [ ] Deploy to staging environment first
- [ ] Verify staging deployment before production
- [ ] Schedule deployment during low-traffic window
- [ ] Monitor deployment closely for first 24 hours

## Post-Deployment

After deploying to production:

- [ ] Verify health endpoints return 200
- [ ] Verify metrics are being collected
- [ ] Verify logs are being aggregated
- [ ] Verify traces are being exported
- [ ] Test critical API endpoints
- [ ] Monitor error rates for 1 hour
- [ ] Verify autoscaling works as expected
- [ ] Update documentation with production URLs
- [ ] Notify stakeholders of successful deployment

## Next Steps

- [Configuration](/docs/configuration) for detailed configuration options
- [Observability](/docs/observability) for monitoring and tracing setup
- [Docker](/docs/docker) for container best practices
- [Kubernetes](/docs/kubernetes) for orchestration details
