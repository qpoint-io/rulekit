const EGRESS_RULE = `-- Backend workloads may reach the shared databases,
-- unless they're running in a dev environment
(
  dst.port in [5432, 6379, 27017]
  and src.pod.namespace in ["api", "billing", "worker"]
  and not (src.pod.labels.env == "dev")
)
-- Internal services, but only over TLS
or (dst.ip in 10.0.0.0/8 and tls.enabled == true)
-- Public container registries
or dst.domain =~ /(^|\\.)(docker\\.io|ghcr\\.io|registry\\.k8s\\.io)$/`

const API_RULE = `-- Read-only API calls from browsers
http.method in ["GET", "HEAD"]
and http.path =~ /^\\/api\\/v[0-9]+\\//
and not (http.headers.user_agent contains "curl")`

const TLS_RULE = `-- Modern TLS to our own domains only
tls.version in ["1.2", "1.3"]
and tls.sni =~ /(^|\\.)example\\.com$/`

const event = (value: unknown) => JSON.stringify(value, null, 2)

/** A policy: one rule and several inputs, each chosen to show one outcome. */
export type Policy = {
  name: string
  description: string
  rule: string
  inputs: SampleInput[]
}

export type SampleInput = {
  id: string
  /** The input JSON. */
  json: string
  label: string
  /** Why this input gets its outcome. */
  description: string
  /** The status this input produces, shown with the legend's icon. */
  expect: "passed" | "failed" | "missing"
}

/** One input together with its policy's rule: what loading an example sets. */
export type Sample = SampleInput & { policy: string; rule: string }

const input = (
  id: string,
  label: string,
  expect: SampleInput["expect"],
  description: string,
  value: unknown
): SampleInput => ({ id, label, expect, description, json: event(value) })

export const POLICIES: Policy[] = [
  {
    name: "Egress policy",
    description: "Which outbound connections backend pods may make",
    rule: EGRESS_RULE,
    inputs: [
      input(
        "db",
        "Billing → Postgres",
        "passed",
        "Production backend reaching a shared database",
        {
          src: { pod: { namespace: "billing", labels: { env: "prod" } } },
          dst: { ip: "10.4.2.17", port: 5432, domain: "postgres.internal" },
          tls: { enabled: true },
        }
      ),
      input(
        "registry",
        "CI → Docker Hub",
        "passed",
        "Image pull from a public registry",
        {
          src: { pod: { namespace: "ci", labels: { env: "prod" } } },
          dst: {
            ip: "104.18.121.25",
            port: 443,
            domain: "registry-1.docker.io",
          },
          tls: { enabled: true },
        }
      ),
      input(
        "devdb",
        "Dev pod → external DB",
        "failed",
        "Dev environment, public IP, no TLS",
        {
          src: { pod: { namespace: "api", labels: { env: "dev" } } },
          dst: { ip: "52.94.233.10", port: 5432, domain: "db.example.com" },
          tls: { enabled: false },
        }
      ),
      input(
        "missing",
        "Incomplete event",
        "missing",
        "No dst.ip or tls, so the TLS branch can’t decide",
        {
          src: { pod: { namespace: "web", labels: { env: "prod" } } },
          dst: { port: 8080, domain: "api.example.com" },
        }
      ),
      input(
        "types",
        "Wrong types",
        "failed",
        "Values the rule can’t compare (string port, list namespace)",
        {
          src: { pod: { namespace: ["api"], labels: { env: "prod" } } },
          dst: { ip: "10.4.2.17", port: "5432", domain: "postgres.internal" },
          tls: { enabled: "yes" },
        }
      ),
    ],
  },
  {
    name: "HTTP API",
    description: "Read-only API calls from browsers, not scripts",
    rule: API_RULE,
    inputs: [
      input(
        "api-ok",
        "Browser GET",
        "passed",
        "GET /api/v2/users from Mozilla",
        {
          http: {
            method: "GET",
            path: "/api/v2/users",
            headers: { user_agent: "Mozilla/5.0" },
          },
        }
      ),
      input("api-curl", "curl POST", "failed", "Write method from a script", {
        http: {
          method: "POST",
          path: "/api/v2/users",
          headers: { user_agent: "curl/8.4" },
        },
      }),
    ],
  },
  {
    name: "TLS",
    description: "Modern TLS to our own domains only",
    rule: TLS_RULE,
    inputs: [
      input(
        "tls-ok",
        "TLS 1.3 to auth.example.com",
        "passed",
        "Current protocol, our domain",
        {
          tls: { version: "1.3", sni: "auth.example.com" },
        }
      ),
      input(
        "tls-old",
        "TLS 1.0 to example.org",
        "failed",
        "Old protocol and someone else’s domain",
        {
          tls: { version: "1.0", sni: "example.org" },
        }
      ),
    ],
  },
]

/** Every input, paired with its policy's rule. */
export const SAMPLES: Sample[] = POLICIES.flatMap((p) =>
  p.inputs.map((i) => ({ ...i, policy: p.name, rule: p.rule }))
)
