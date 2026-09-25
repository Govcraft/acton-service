# Security policy

## Reporting a vulnerability

Please do not report security vulnerabilities through public GitHub issues, discussions or pull requests.

Report them privately through GitHub instead:
**[Report a vulnerability](https://github.com/Govcraft/acton-service/security/advisories/new)**.
You can also reach it from the repository's **Security** tab. Only the maintainers can see what you send.

Please include as much of the following as you can:

- the affected version, and the crate or component
- the configuration needed to reproduce it
- step-by-step reproduction, ideally a minimal example
- the impact: what an attacker can read, change or disrupt
- a suggested fix, if you have one

## What to expect

- **Acknowledgement** within 3 business days.
- **Initial assessment** within 10 business days. This covers whether we can reproduce the report, how severe we think it is, and what happens next.
- **Updates** at least every two weeks until the report is resolved.
- **A fix and an advisory.** Once a fix is released, we publish a GitHub security advisory and request a CVE where appropriate. With your permission, we credit you in it.

We ask that you keep the details private until the advisory is published, or for 90 days from your report, whichever comes first. If we need longer for a complex fix, we will tell you why and agree a date with you.

## Supported versions

Security fixes go into the latest release. Older versions do not receive backports, so please upgrade to get a fix.

| Version | Supported |
|---|---|
| 0.44.x (latest release) | Yes |
| earlier than 0.44 | No, please upgrade |

## Scope

In scope:

- the `acton-service` crate and the `acton-cli` crate
- authentication (PASETO, JWT, API keys, OAuth/OIDC), sessions and CSRF, Cedar authorization, TLS and mTLS, rate limiting, audit logging and configuration loading
- default settings that leave a service less secure than the documentation says

Out of scope:

- a weakness that exists only because an application disabled a protection or configured it against the documentation
- vulnerabilities in dependencies with no acton-service-specific impact (please report those upstream; tell us if they affect acton-service)

If you are unsure whether something counts, report it privately anyway. We would rather hear about it.
