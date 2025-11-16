---
title: Kubernetes Deployment
nextjs:
  metadata:
    title: Kubernetes Deployment
    description: Deploy acton-service applications to Kubernetes with health probes, configuration, and production-ready manifests.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Deploy acton-service applications to Kubernetes with built-in health checks, configuration management, and horizontal scaling support.

## Deployment Manifest

Create a Kubernetes Deployment with liveness and readiness probes:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  replicas: 3
  selector:
    matchLabels:
      app: my-service
  template:
    metadata:
      labels:
        app: my-service
    spec:
      containers:
      - name: my-service
        image: my-service:latest
        ports:
        - containerPort: 8080
        env:
        - name: ACTON_SERVICE_PORT
          value: "8080"
        - name: ACTON_DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: db-credentials
              key: url
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
```

## Service Configuration

Expose your deployment with a Service:

```yaml
apiVersion: v1
kind: Service
metadata:
  name: my-service
spec:
  selector:
    app: my-service
  ports:
  - protocol: TCP
    port: 80
    targetPort: 8080
  type: ClusterIP
```

## Health Probes

acton-service provides automatic health endpoints for Kubernetes orchestration:

### Liveness Probe

The `/health` endpoint verifies the service is alive and responsive:

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 8080
  initialDelaySeconds: 30
  periodSeconds: 10
  timeoutSeconds: 3
  failureThreshold: 3
```

If this probe fails, Kubernetes will restart the pod.

### Readiness Probe

The `/ready` endpoint checks if the service can accept traffic, including dependency health:

```yaml
readinessProbe:
  httpGet:
    path: /ready
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 5
  timeoutSeconds: 3
  failureThreshold: 3
```

If this probe fails, Kubernetes will remove the pod from service load balancers.

## Environment Configuration

### Using Secrets

Store sensitive configuration in Kubernetes Secrets:

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: db-credentials
type: Opaque
stringData:
  url: postgres://user:password@postgres:5432/mydb
```

Reference secrets in your deployment:

```yaml
env:
- name: ACTON_DATABASE_URL
  valueFrom:
    secretKeyRef:
      name: db-credentials
      key: url
- name: ACTON_REDIS_URL
  valueFrom:
    secretKeyRef:
      name: redis-credentials
      key: url
```

### Using ConfigMaps

Store non-sensitive configuration in ConfigMaps:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-service-config
data:
  RUST_LOG: "info"
  ACTON_SERVICE_PORT: "8080"
  ACTON_GRPC_ENABLED: "true"
```

Reference in deployment:

```yaml
envFrom:
- configMapRef:
    name: my-service-config
```

## Resource Limits

Set resource requests and limits for predictable scheduling:

```yaml
spec:
  containers:
  - name: my-service
    resources:
      requests:
        memory: "256Mi"
        cpu: "250m"
      limits:
        memory: "512Mi"
        cpu: "500m"
```

## Horizontal Pod Autoscaling

Enable automatic scaling based on CPU or memory usage:

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: my-service-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: my-service
  minReplicas: 3
  maxReplicas: 10
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
```

## Ingress Configuration

Expose your service externally with an Ingress:

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: my-service-ingress
  annotations:
    cert-manager.io/cluster-issuer: "letsencrypt-prod"
spec:
  tls:
  - hosts:
    - api.example.com
    secretName: my-service-tls
  rules:
  - host: api.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: my-service
            port:
              number: 80
```

## Complete Manifest Generation

Use the acton CLI to generate complete Kubernetes manifests:

```bash
acton service generate deployment --hpa --monitoring --ingress
```

This generates:
- Deployment with health probes
- Service configuration
- HorizontalPodAutoscaler
- ServiceMonitor for Prometheus
- Ingress with TLS

## Rolling Updates

Configure rolling update strategy for zero-downtime deployments:

```yaml
spec:
  replicas: 3
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  template:
    # ... container spec
```

Deploy updates:

```bash
kubectl set image deployment/my-service my-service=my-service:v2.0.0
kubectl rollout status deployment/my-service
```

## Service Mesh Integration

For Istio or Linkerd, add sidecar injection:

```yaml
metadata:
  annotations:
    sidecar.istio.io/inject: "true"
```

## Monitoring with Prometheus

Create a ServiceMonitor for Prometheus Operator:

```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: my-service
spec:
  selector:
    matchLabels:
      app: my-service
  endpoints:
  - port: http
    path: /metrics
    interval: 30s
```

acton-service automatically exposes OpenTelemetry metrics at `/metrics`.

## Pod Disruption Budget

Ensure availability during node maintenance:

```yaml
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: my-service-pdb
spec:
  minAvailable: 2
  selector:
    matchLabels:
      app: my-service
```

## Troubleshooting

### Check Pod Status

```bash
kubectl get pods -l app=my-service
kubectl describe pod <pod-name>
```

### View Logs

```bash
kubectl logs -l app=my-service --tail=100 -f
```

### Check Health Endpoints

```bash
kubectl port-forward svc/my-service 8080:80
curl http://localhost:8080/health
curl http://localhost:8080/ready
```

### Debug Failed Probes

```bash
kubectl describe pod <pod-name> | grep -A 10 "Liveness\|Readiness"
```

## Next Steps

- [Production Checklist](/docs/production) for deployment best practices
- [Configuration](/docs/configuration) for advanced settings
- [Observability](/docs/observability) for monitoring and tracing
