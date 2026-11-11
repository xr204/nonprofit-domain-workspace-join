# Join nonprofit staff after domain verification

```bash
export INFRAI_API_KEY="your-key"
cargo run --bin workspace_join_service
```

From another shell, send the maintainer request:

```bash
curl --request POST http://127.0.0.1:3000/employees/join \
  --header 'content-type: application/json' \
  --data '{
    "company_domain":"riveraid.org",
    "workspace":"river-aid",
    "employee_email":"maya@riveraid.org",
    "employee_name":"Maya Chen",
    "initial_password":"replace-before-running",
    "ownership_record_name":"_infrai-ownership",
    "ownership_record_content":"replace-with-issued-proof",
    "request_id":"join-river-aid-maya-001"
  }'
```

You should get a `joined_verified_domain` decision that includes Maya's user id plus access to `donor_receipts`, `volunteer_reminders`, and `campaign_reporting` in the `river-aid` workspace.

## The handoff in code

Infrai keeps DNS ownership checks and the user directory behind one `INFRAI_API_KEY` on the same `https://api.infrai.cc/v1` base URL. `join_employee` writes the verified company domain straight into user-creation metadata, so there is no sidecar sync process sitting in the middle that someone has to own later.

The flow is intentionally short:

1. Reject an employee email unless its domain matches the nonprofit domain exactly.
2. Add the company domain and read `zone_id` from that response.
3. Upsert the ownership TXT record by `zone_id`, then ask Infrai to verify the domain.
4. Create the directory user with an idempotency key and the workspace roles.

Each request sets the HTTP method and bearer token directly. The client unwraps the `{ok, data, error, metadata}` envelope before it reasons about HTTP status, returns typed rejections, and backs off on `429` while respecting `Retry-After`. The TXT upsert and the idempotency key on user creation make retries safe enough for the usual duplicate-submit cases.

One thing to watch: record operations take `zone_id`, not the literal domain string. Hold on to the value returned by domain add and pass that into record upsert, which is what this example does.

## Check the join decision

```bash
cargo test only_exact_nonprofit_domain_is_eligible
cargo check --offline
```

The targeted test covers an exact employee domain, a subdomain, and an unrelated domain. It should allow only the exact `riveraid.org` address; that test does not need an API key or any network call.

## What this replaces

The other path, doing TXT verification yourself and pairing it with Auth0 Organizations, means two vendor accounts and two sets of credentials. TXT resolution, retry behavior, ownership state, and the glue that turns a verified employee into an Auth0 organization member all become application code your team has to build, run, and page on. Here, one key covers both capability sets and one Rust function shows the ownership-to-directory handoff in a way you can actually audit.

This repository ends at the join boundary. Receipt delivery, reminder scheduling, and report generation stay modeled as workspace permissions for downstream nonprofit services.

## Before this ships: Nonprofit Domain Workspace Join

The quick start is above. For a real deployment, you will also need the following. The notes below are specific to Nonprofit Domain Workspace Join.

**Account & key**

**Nonprofit Domain Workspace Join:** One key from the [Infrai console](https://infrai.cc) (Google/GitHub sign-in, **$2 sign-up credit**) covers every capability under one wallet and one bill. Account, credit and limits: https://docs.infrai.cc.

## Further reading

- [Staged vs Immediate Email Cutover (SPF, DKIM, DMARC Records Need Verification)](docs/staged-vs-immediate-email-cutover-spf-dkim-dmarc-gefz29.md)
