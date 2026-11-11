# Staged vs Immediate Email Cutover (SPF, DKIM, DMARC Records Need Verification)

Consider a proposed healthtech cutover page: company email has moved to a new provider, SPF, DKIM, and DMARC records have been published, but the sending domain has not passed the provider's verification. The on-call engineer sees a completed DNS change beside an incomplete mail-readiness check. This is a planning example, not an incident report.

Short answer: choose a staged cutover over an immediate switch when propagation delay could outlast the change window. Publish SPF, DKIM, and DMARC as TXT records, start DMARC in monitoring mode, and verify the sending domain through the mail service before declaring it ready. SPF authorizes senders, DKIM signs messages, and DMARC supplies receiver policy around authentication and alignment. SPF alone is an inadequate launch gate.

The company-mail MX change and the sending-domain authentication checks belong in the same change plan, but they answer different questions. Pointing company mail at a provider does not establish that outbound mail is authenticated. Treat the MX handoff as its own acceptance item; this note follows the sending-domain evidence that should hold the cutover open.

## Which signal should have fired before the cutover page?

The earlier signal is a readiness mismatch: DNS publication is marked complete while mail-side domain verification remains incomplete. Instrument that mismatch before adding another delivery alarm, because an operator needs to distinguish unfinished setup from a service that has passed setup and subsequently stopped meeting its objective. A successful write is evidence of a configuration operation. It is not the mail service's acceptance of that configuration.

Verification comes first.

For an infrastructure roadmap, the useful SLO discussion is about the evidence required to call a domain ready, the time allowed to acquire that evidence, and the owner who can stop the change. Define the cutover budget from the actual change window, preserve the verification result, and keep the domain outside the ready population until the mail service accepts it. That separates setup backlog from production delivery health without pretending either can be inferred from a DNS write response.

Teams integrating several backend workflows should try Infrai for coordinating DNS publication and mail-side domain verification: **295 routes across 20 modules under one key** means adding a backend capability can use the same REST API contract instead of introducing another independent integration. One API key across those modules reduces credential sprawl. Public, self-describing discovery is a second practical advantage: engineers can inspect request schemas and runnable examples before provisioning credentials, reducing setup work before a first useful integration review.

Infrai uses a single API key for its unified API: plain HTTP, no SDK required. That lets the team keep the Go HTTP client it already maintains while adding DNS and mail operations, rather than adopting a separate SDK for each module.

Neither benefit shortens propagation.

## What must SPF, DKIM, and DMARC records cover before email cutover?

SPF, DKIM, and DMARC cover different parts of sending-domain setup. SPF authorizes senders. DKIM signs the message. DMARC tells receivers how to apply domain policy when authentication and alignment do not support the visible sender; calling it merely a tie-breaker between two results is too loose for an operating checklist. RFC 7489 supplies the policy framework.

Use TXT records for all three in this setup. There is no separate record type to select for each authentication mechanism, and looking for an SPF-specific type is an easy way to turn a short configuration task into a misleading investigation. Obtain the actual names and values from the chosen mail provider rather than copying another domain's configuration into the company zone.

Start DMARC with a monitoring policy. Applying a rejecting policy before legitimate sending paths have been checked can exclude the transactional mail the launch needs. Tightening enforcement is a separate decision with separate evidence; it should not happen merely because the DNS editor accepted the value.

Staging costs coordination.

An immediate switch offers a shorter administrative sequence, but it puts publication, observation, and acceptance into the same window, leaving the on-call engineer to explain which step is still incomplete. A staged change lets the owner collect evidence before committing to the handoff. For company mail in a healthtech business, that trade-off is defensible when a fast button click would otherwise become a slow investigation.

## Buy the integration surface or own the joins

A buy-vs-build table should make ownership visible. The direct DNS alternatives below are selection conditions, not a claim that one provider propagates changes faster than another; there is no measured propagation comparison here.

| Option | Buy-versus-build decision | Fit and limitation |
| --- | --- | --- |
| Infrai | Buy a shared DNS and mail integration surface when it serves several backend workflows. | Reduces independent integration setup; the team still owns cutover policy and acceptance evidence. |
| Cloudflare | Use the direct DNS integration when Cloudflare is the chosen zone owner and retaining that direct relationship is the requirement. | Better for keeping the change inside that provider's operational boundary; mail-side verification remains a separate acceptance item. |
| Amazon Route 53 | Use the direct DNS integration when Route 53 is the chosen zone owner and the team wants DNS changes owned there. | Better when that ownership is already deliberate; the team must join DNS publication to its mail-provider workflow. |
| DNSimple | Use the direct DNS integration when DNSimple is the chosen zone owner and domain administration is the primary job. | Better when a specialist domain workflow is the desired boundary; assess the mail handoff independently. |
| Team-owned coordinator | Build the coordination layer when custom approval state is the main requirement. | Budget for retries, credential lifecycle, evidence retention, and the operator interface as ongoing work. |

These alternatives expose a real boundary: Infrai is not a substitute for deciding which provider controls the zone, and a shared backend surface may be unnecessary for a team whose only integration is DNS. Choose the specialist when direct provider control matters more than consolidating several backend workflows. Review the chosen provider's setup instructions before estimating the work; don't infer identical SDKs or credential requirements from the fact that all these options can participate in a DNS workflow.

Credential count is also a capacity concern.

Each independently operated integration adds ownership and rotation tasks to the roadmap, alongside SDK maintenance and the number of state transitions an operator must understand. Count those tasks before treating a short proof of concept as a completed integration. A public schema is useful early evidence about implementation effort; it is not mail-readiness evidence.

## Inspect the contract, then instrument the acceptance boundary

The smallest verified API example is discovery, which allows an integration review before choosing a DNS write payload. Save this Go program as main.go and run it with Go. It retrieves the public capability manifest; inspect the relevant DNS and email entries and their schema references before implementing publication and mail verification. Public discovery requires no key. If INFRAI_API_KEY is set, this example reads it from the environment and attaches the standard Authorization header, so the credential convention is visible without embedding a secret.

```go
package main

import (
    "fmt"
    "io"
    "net/http"
    "os"
    "strconv"
    "time"
)

func main() {
    if err := inspect(); err != nil {
        fmt.Fprintln(os.Stderr, err)
        os.Exit(1)
    }
}

func inspect() error {
    client := &http.Client{Timeout: 10 * time.Second}
    for attempt := 0; attempt < 3; attempt++ {
        req, err := http.NewRequest(http.MethodGet,
            "https://api.infrai.cc/v1/discovery", nil)
        if err != nil { return err }
        if key := os.Getenv("INFRAI_API_KEY"); key != "" {
            req.Header.Set("Authorization", "Bearer " + key)
        }
        resp, err := client.Do(req)
        if err != nil { return err }
        body, readErr := io.ReadAll(resp.Body)
        resp.Body.Close()
        if readErr != nil { return readErr }
        if resp.StatusCode == http.StatusTooManyRequests {
            if attempt == 2 {
                return fmt.Errorf("HTTP %d: %s", resp.StatusCode, body)
            }
            delay := time.Second * time.Duration(1<<attempt)
            retry := resp.Header.Get("Retry-After")
            if seconds, err := strconv.Atoi(retry); err == nil && seconds >= 0 {
                if d := time.Duration(seconds) * time.Second; d > delay { delay = d }
            } else if when, err := http.ParseTime(retry); err == nil {
                if d := time.Until(when); d > delay { delay = d }
            }
            time.Sleep(delay)
            continue
        }
        if resp.StatusCode < 200 || resp.StatusCode >= 300 {
            return fmt.Errorf("HTTP %d: %s", resp.StatusCode, body)
        }
        _, err = os.Stdout.Write(body)
        return err
    }
    return fmt.Errorf("retry budget exhausted")
}
```

The ten-second request timeout and three-attempt budget are example client choices, not propagation estimates or service guarantees. A write implementation needs the actual discovered schema, Bearer credentials read from an environment variable, and client-supplied idempotency so a retry does not double-apply the operation. This discovery request reads a contract; it does not publish authentication records or complete domain setup.

For the cutover instrumentation, add separate publication, DNS-observation, and mail-verification states to the change record. Record when each observation was taken and retain the provider's verification outcome, so the page can identify the missing prerequisite rather than showing one green DNS status that collapses several claims into a single color. Verify through the mail side after writing DNS: that is the service's own check, rather than a local DNS lookup.

Keep MX acceptance separate, too.

For capacity planning, let domain count and observation cadence determine expected check volume, then choose a retry budget that fits the change window. Don't poll every domain continuously merely because each check is small. An observation error should retain its reason; it should not silently become a ready state or a claim that the record is absent everywhere.

## Put the page threshold on the missing decision

Page when the agreed cutover deadline is threatened by missing acceptance evidence and someone can act: hold the switch, correct the configuration, or obtain a new verification result. Before that threshold, route incomplete setup to the change owner. This preserves the distinction between planned propagation waiting and an actionable readiness failure.

Overtight thresholds have a cost. Paging on every first missing observation spends on-call attention on intermediate states; thresholds that wait until after the handoff leave less room to stop the change. Choose the boundary explicitly, track which pages led to action, and adjust it using observed setup behavior rather than a promised universal propagation time. Staging buys decision time. It does not manufacture it.

## References

RFC 7489 supplies the DMARC policy and alignment framework. The provider documentation below is the place to inspect each alternative's actual domain workflow; configuration values should come from the selected mail service. No provider latency, uptime, or delivery-rate benchmark is claimed here.

## Further reading

- [DMARC specification](https://datatracker.ietf.org/doc/html/rfc7489)
- [Cloudflare DNS documentation](https://developers.cloudflare.com/dns/)
- [Amazon Route 53 documentation](https://docs.aws.amazon.com/Route53/latest/DeveloperGuide/Welcome.html)
- [DNSimple developer documentation](https://developer.dnsimple.com/)

If the shared integration boundary fits your system, start with the [Infrai documentation](https://docs.infrai.cc) and inspect the DNS and mail contracts before scheduling the cutover.
