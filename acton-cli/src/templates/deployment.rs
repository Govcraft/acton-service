pub struct DeploymentConfig {
    pub service_name: String,
    pub namespace: Option<String>,
    pub replicas: u32,
    pub image: String,
    pub image_tag: String,
    pub memory_limit: String,
    pub cpu_limit: String,
    pub enable_hpa: bool,
    pub enable_monitoring: bool,
    pub enable_ingress: bool,
    pub enable_tls: bool,
    pub environment: Option<String>,
}

pub fn generate_dockerfile(service_name: &str) -> String {
    format!(
r#"# Multi-stage build for {}

# Stage 1: Build
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# Stage 2: Runtime
FROM alpine:3.19

RUN apk add --no-cache ca-certificates && \
    addgroup -g 1000 appuser && \
    adduser -D -u 1000 -G appuser appuser

WORKDIR /app
COPY --from=builder /build/target/release/{} /app/

USER appuser
EXPOSE 8080 9090

CMD ["/app/{}"]
"#,
        service_name, service_name, service_name
    )
}

pub fn generate_dockerignore() -> String {
r#"target/
.git/
.gitignore
*.md
.env
.env.local
config.local.toml
"#.to_string()
}

pub fn generate_k8s_deployment(config: &DeploymentConfig) -> String {
    let namespace = config.namespace.as_deref().unwrap_or("default");
    let env_label = config.environment.as_ref().map(|e| format!("\n    environment: {}", e)).unwrap_or_default();

    format!(
r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {service_name}
  namespace: {namespace}
  labels:
    app: {service_name}
    version: v1{env_label}
spec:
  replicas: {replicas}
  selector:
    matchLabels:
      app: {service_name}
  template:
    metadata:
      labels:
        app: {service_name}
        version: v1{env_label}
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "9090"
        prometheus.io/path: "/metrics"
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        fsGroup: 1000
      containers:
      - name: {service_name}
        image: {image}:{image_tag}
        imagePullPolicy: IfNotPresent
        ports:
        - name: http
          containerPort: 8080
          protocol: TCP
        - name: metrics
          containerPort: 9090
          protocol: TCP
        env:
        - name: RUST_LOG
          value: "info"
        - name: SERVICE_NAME
          value: "{service_name}"
        resources:
          requests:
            memory: "{memory_request}"
            cpu: "{cpu_request}"
          limits:
            memory: "{memory_limit}"
            cpu: "{cpu_limit}"
        livenessProbe:
          httpGet:
            path: /health
            port: http
          initialDelaySeconds: 30
          periodSeconds: 10
          timeoutSeconds: 5
          failureThreshold: 3
        readinessProbe:
          httpGet:
            path: /ready
            port: http
          initialDelaySeconds: 5
          periodSeconds: 5
          timeoutSeconds: 3
          failureThreshold: 2
        securityContext:
          allowPrivilegeEscalation: false
          readOnlyRootFilesystem: true
          capabilities:
            drop:
            - ALL
"#,
        service_name = config.service_name,
        namespace = namespace,
        env_label = env_label,
        replicas = config.replicas,
        image = config.image,
        image_tag = config.image_tag,
        memory_request = calculate_request(&config.memory_limit, 0.75),
        cpu_request = calculate_request(&config.cpu_limit, 0.5),
        memory_limit = config.memory_limit,
        cpu_limit = config.cpu_limit,
    )
}

pub fn generate_k8s_service(config: &DeploymentConfig) -> String {
    let namespace = config.namespace.as_deref().unwrap_or("default");

    format!(
r#"apiVersion: v1
kind: Service
metadata:
  name: {service_name}
  namespace: {namespace}
  labels:
    app: {service_name}
spec:
  type: ClusterIP
  ports:
  - name: http
    port: 80
    targetPort: http
    protocol: TCP
  - name: metrics
    port: 9090
    targetPort: metrics
    protocol: TCP
  selector:
    app: {service_name}
"#,
        service_name = config.service_name,
        namespace = namespace,
    )
}

pub fn generate_k8s_hpa(config: &DeploymentConfig) -> String {
    let namespace = config.namespace.as_deref().unwrap_or("default");

    format!(
r#"apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: {service_name}
  namespace: {namespace}
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: {service_name}
  minReplicas: {min_replicas}
  maxReplicas: {max_replicas}
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
  behavior:
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
      - type: Percent
        value: 50
        periodSeconds: 15
    scaleUp:
      stabilizationWindowSeconds: 0
      policies:
      - type: Percent
        value: 100
        periodSeconds: 15
      - type: Pods
        value: 2
        periodSeconds: 15
      selectPolicy: Max
"#,
        service_name = config.service_name,
        namespace = namespace,
        min_replicas = config.replicas,
        max_replicas = config.replicas * 3,
    )
}

pub fn generate_k8s_ingress(config: &DeploymentConfig) -> String {
    let namespace = config.namespace.as_deref().unwrap_or("default");
    let tls_section = if config.enable_tls {
        format!(
r#"  tls:
  - hosts:
    - {service_name}.example.com
    secretName: {service_name}-tls
"#,
            service_name = config.service_name
        )
    } else {
        String::new()
    };

    format!(
r#"apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: {service_name}
  namespace: {namespace}
  annotations:
    nginx.ingress.kubernetes.io/rewrite-target: /
    nginx.ingress.kubernetes.io/ssl-redirect: "{ssl_redirect}"
spec:
{tls_section}  rules:
  - host: {service_name}.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: {service_name}
            port:
              number: 80
"#,
        service_name = config.service_name,
        namespace = namespace,
        ssl_redirect = if config.enable_tls { "true" } else { "false" },
        tls_section = tls_section,
    )
}

pub fn generate_service_monitor(config: &DeploymentConfig) -> String {
    let namespace = config.namespace.as_deref().unwrap_or("default");

    format!(
r#"apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: {service_name}
  namespace: {namespace}
  labels:
    app: {service_name}
spec:
  selector:
    matchLabels:
      app: {service_name}
  endpoints:
  - port: metrics
    interval: 30s
    path: /metrics
"#,
        service_name = config.service_name,
        namespace = namespace,
    )
}

fn calculate_request(limit: &str, ratio: f32) -> String {
    // Simple parser for memory/CPU limits
    if limit.ends_with("Mi") {
        if let Ok(value) = limit.trim_end_matches("Mi").parse::<u32>() {
            return format!("{}Mi", (value as f32 * ratio) as u32);
        }
    } else if limit.ends_with('m') {
        if let Ok(value) = limit.trim_end_matches('m').parse::<u32>() {
            return format!("{}m", (value as f32 * ratio) as u32);
        }
    }
    limit.to_string()
}
