# ICP: realtime updates & static frontend delivery — source-verified report

Legend used throughout:
- **[SPEC]** = normative IC specification text.
- **[DOCS]** = official DFINITY developer documentation.
- **[STAFF]** = statement by a DFINITY employee on the official forum (not formal docs).
- **[VENDOR]** = project README / maintainer claim about their own product.
- **[U] UNVERIFIED** = could not confirm from a primary source.

---

## 1. Native WebSocket ingress from a browser to a canister

**No. The Internet Computer does not support a browser-to-canister WebSocket natively.**

Evidence:

1. The normative HTTPS interface of the IC defines exactly five endpoints — `…/call`, `…/read_state`, `…/query`, `/api/v2/status` — all of them plain POST (or GET for status) requests with CBOR bodies. There is no `Upgrade: websocket` endpoint, no `ws://`/`wss://` path, and no canister-facing socket primitive anywhere in the spec. ([HTTP interface, ICP interface spec](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/))
2. DFINITY's own WebSocket proof-of-concept repository states verbatim: *"At the moment, WebSockets are not supported for dapps on the Internet Computer and developers need to resort to work-arounds in the frontend to enable a similar functionality."* ([dfinity/ic-websocket-poc](https://github.com/dfinity/ic-websocket-poc))
3. The main third-party implementation says the same: *"the Internet Computer does not natively support WebSocket connections and developers need to resort to work-arounds."* ([omnia-network/ic-websocket-gateway README](https://github.com/omnia-network/ic-websocket-gateway))
4. The stated architectural reason is replication: *"the gateway is needed as a WebSocket is a one-to-one connection between client and server, but the Internet Computer does not support that due to its replicated nature."* ([dfinity/ic-websocket-poc](https://github.com/dfinity/ic-websocket-poc), repeated in the [Omnia README](https://github.com/omnia-network/ic-websocket-gateway))
5. DFINITY's announcement of the PoC confirms the design is a **gateway plus polling**, not a new ingress protocol: *"It establishes WebSocket connections with canisters through update calls and uses polling to obtain the latest messages and deliver them to the client."* It also states the PoC *"is not usable in production."* ([DFINITY forum, WebSockets on the IC – A proof-of-concept](https://forum.dfinity.org/t/websockets-on-the-ic-a-proof-of-concept/20836))

**The one WebSocket endpoint the IC does officially expose is not canister ingress.** API boundary nodes stream their *access logs* over WebSocket:

```
wss://{api_bn_domain}/logs/canister/{canister_id}
```

This is an observability feed served by boundary-node infrastructure. The docs emphasise that *"Each API BN only streams logs for requests it receives. To get complete log coverage, you must connect to all API BNs."* ([Streaming canister access logs](https://internetcomputer.org/docs/building-apps/advanced/canister-access-logs); sample client: [dfinity/ic-bn-logs, Apache-2.0](https://github.com/dfinity/ic-bn-logs)) This is outbound-from-BN, not a canister accepting a socket.

### Roadmap / status statements

- **[VENDOR/community] 2023-07 roadmap:** the long-term goal is that *"the WebSocket Gateway should be integrated into the boundary nodes and become part of the official IC specification as a new endpoint."* But: *"this is not on Dfinity's short term roadmap and it is not possible for external developers to contribute to the boundary nodes."* The interim plan was a standalone service, then a "trusted" gateway (V1, Aug 2023), then a "trustless" one (V2, Oct 2023), then privacy via vetKD. ([IC WebSocket: Roadmap](https://forum.dfinity.org/t/ic-websocket-roadmap/21503))
- **[VENDOR/community] 2023-10:** integration into API boundary nodes is not short-term because *"this decision would have to be proposed and adopted by the community, as API Boundary Nodes will soon be controlled by the NNS"*, and *"only the API Boundary Node image can run on the server which the NNS will elect as a Boundary Node."* The claim is that HTTP Gateway and WS Gateway would instead both run on separate servers *"outside" of the IC*, with DFINITY *"hopefully"* also hosting a WS Gateway. ([IC WebSocket: Stable Release, post 6](https://forum.dfinity.org/t/ic-websocket-stable-release/23872/6))
- **[SPEC-adjacent] The HTTP Gateway Protocol does have a "streaming" mechanism, but it is not push.** It exists *"to transfer further chunks of the body data from the canister to the HTTP Gateway, to overcome the message limit of the Internet Computer"*, and the gateway may either buffer the whole body or pass chunks on. It also has an "upgrade to update call" flag so a canister can handle a request as an update call. Neither creates a persistent server→client channel. ([HTTP gateway protocol spec](https://docs.internetcomputer.org/references/http-gateway-protocol-spec/))
- **[U] Could not verify** any official DFINITY/NNS statement dated 2024–2026 that commits to native WebSocket ingress for canisters. The DFINITY PoC repo is now archived (read-only), pushed 2025-08-28 ([GitHub API metadata](https://github.com/dfinity/ic-websocket-poc)), which is at least consistent with it not being an actively developed protocol feature, but absence of a statement is not a statement of absence.

---

## 2. How a browser actually reaches a canister

### The actual path

Official flow, verbatim from the docs ([Edge infrastructure](https://docs.internetcomputer.org/concepts/edge-infrastructure/)):

1. Browser makes a normal HTTPS request to e.g. `https://<canister-id>.icp.net`; it has no awareness that the site runs on ICP.
2. The **HTTP gateway** receives it and translates it into a query call to the canister's `http_request` method, putting path/headers/body into the payload.
3. An **API boundary node** receives the IC API call and forwards it to a replica on the subnet hosting the target canister.
4. The canister executes `http_request` as a **query**, builds an HTTP response, returns it.
5. The gateway verifies the certificate and constructs a standard HTTP response.
6. The browser renders it.

API boundary nodes *"receive IC API requests and route them to nodes on the appropriate subnet"*, and additionally do dynamic routing, load balancing, caching of some query responses, and security enforcement. They are NNS-governed infrastructure, run by independent node providers, currently ~20 deployed worldwide, all running a service called `ic-boundary`. ([Edge infrastructure](https://docs.internetcomputer.org/concepts/edge-infrastructure/); dashboard list: [IC dashboard, API boundary nodes](https://dashboard.internetcomputer.org/nodes?s=100&type=ApiBoundary))

### HTTP gateway protocol and the boundary node's role

The **[HTTP Gateway Protocol specification](https://docs.internetcomputer.org/references/http-gateway-protocol-spec/)** defines the translation layer. It is explicitly implementation-independent and states that a gateway *"could be a stand-alone proxy, it could be implemented in web browsers (natively, via a plugin or a service worker) or in other ways."* Its documented steps are: resolve canister ID → Candid-encode the HTTP request → **query call to `http_request`** → decode response → if the canister asked for it, resend via **update call to `http_request_update`** → if applicable, fetch further body data via **streaming query calls** → validate the certificate → return the decoded response.

The API boundary node is the part that actually holds the IC connection: it receives the IC API call and routes it to the subnet. So the full chain is: **browser → HTTP gateway → API boundary node → subnet replica → single canister execution**. ([Edge infrastructure](https://docs.internetcomputer.org/concepts/edge-infrastructure/))

### Is the gateway trusted for query responses? — important correction to the premise

Query calls are answered by a single replica and *"do not go through consensus and are not automatically certified."* Certification fixes this: the canister commits a hash to the subnet's certified state during an update call, the subnet's threshold-BLS key signs it, and the query response carries the certificate plus a Merkle witness in `IC-Certificate` / `IC-Certificate-Expression` headers. ([Response certification](https://docs.internetcomputer.org/guides/frontends/certification/); [Edge infrastructure, "Asset certification"](https://docs.internetcomputer.org/concepts/edge-infrastructure/))

But **on the browser path it is the gateway, not the browser, that verifies**. The docs are explicit:

> *"A proof only helps if somebody checks it, and in a browser that somebody is the HTTP gateway… A browser can't verify an IC certificate on its own, so it delegates that check. **Choosing the gateway is the whole trust decision.**"* ([Under the hood](https://docs.internetcomputer.org/guides/frontends/static-site/how-it-works/))

So the accurate statement is:
- A **gateway cannot forge a response that a verifying party will accept.** Verification chains to the ICP root public key and requires ≥2/3 of subnet nodes via chain-key cryptography, so *"a certified response represents network-level consensus, not a single node's assertion."* ([Edge infrastructure](https://docs.internetcomputer.org/concepts/edge-infrastructure/))
- But a **malicious or misconfigured gateway can simply not verify** and serve unverified content. Two documented escape hatches exist: the `raw` hostname (`<canister-id>.raw.icp.net`) where *"the canister still attaches the certificate; the gateway discards it"*, and the protocol's `no_certification` expression. ([Response certification](https://docs.internetcomputer.org/guides/frontends/certification/); [HTTP gateway protocol spec](https://docs.internetcomputer.org/references/http-gateway-protocol-spec/))
- A **client that talks to a canister directly (not via an HTTP gateway), e.g. through an agent, can verify the certificate itself** — via `read_state`, or by submitting a query method as a `call` request to get a certified response. ([HTTP interface spec](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/))
- **A canister cannot reliably refuse `raw` requests**, because its only clue is the unauthenticated `Host` header; the docs call that check *"wrong in both directions."* ([Under the hood](https://docs.internetcomputer.org/guides/frontends/static-site/how-it-works/))

The net for a developer: the content path is *integrity-protected by default*, but the **gateway is fully trusted for availability and for observing traffic**, and any client that links to a non-verifying hostname gets no cryptographic assurance at all.

### Can a developer self-host the gateway? — Yes

- **[DOCS]** *"HTTP gateways are not part of ICP itself and can be operated by anyone. This open model encourages a diverse set of gateways, enhancing redundancy and availability."* ([Edge infrastructure](https://docs.internetcomputer.org/concepts/edge-infrastructure/))
- **[Name/URL/license]** `dfinity/ic-gateway` — *"the core service of the HTTP gateway that allows direct HTTP access to the canisters"*. **Apache License 2.0.** Actively maintained (last push 2026-09-16 per the GitHub API). Includes TLS termination via ACME, caching, denylist, load shedding, and can use the boundary-node discovery library (`--ic-use-discovery`). Distributed as a release binary, Docker image `ghcr.io/dfinity/ic-gateway`, or built with `cargo`. External code contributions are **not** accepted. ([dfinity/ic-gateway](https://github.com/dfinity/ic-gateway))
- **[Name/URL/license]** `dfinity/ic-http-gateway-protocol` — *"Monorepo including building blocks, reference implementations and examples for HTTP Gateway Protocol implementations"* (notably `@dfinity/http-canister-client`). **Apache License 2.0.** Actively maintained. The docs point to it as *"the main implementation of the HTTP Gateway Protocol."* ([dfinity/ic-http-gateway-protocol](https://github.com/dfinity/ic-http-gateway-protocol); linked from [Edge infrastructure](https://docs.internetcomputer.org/concepts/edge-infrastructure/))
- **[Note]** The older standalone repo `dfinity/ic-http-gateway` now returns HTTP 404 from the GitHub API; current self-hostable tooling is `ic-gateway` plus the `ic-http-gateway-protocol` monorepo. Related official components named in that monorepo: [response-verification](https://github.com/dfinity/response-verification), the [IC service worker](https://github.com/dfinity/ic/tree/master/typescript/service-worker), `icx_proxy`, and the [desktop HTTP proxy](https://github.com/dfinity/http-proxy).

---

## 3. Open-source WebSocket gateways for ICP canisters

### `ic-websocket-gateway` (Omnia Network)

| Property | Value | Source |
|---|---|---|
| Repository | https://github.com/omnia-network/ic-websocket-gateway | GitHub |
| License | **MIT** (`Copyright (c) 2023 Omnia Team`) | [LICENSE](https://github.com/omnia-network/ic-websocket-gateway/blob/main/LICENSE), confirmed via GitHub API `spdx_id: MIT` |
| Language | Rust | GitHub |
| Stars / forks | 34 / 4 | GitHub API |
| Archived? | No | GitHub API |
| Latest release | **v1.4.6, 2025-02-01** | GitHub releases API |
| Last commit | **2025-06-17** | GitHub API `pushed_at` |
| Related SDKs | `ic-websocket-sdk-js` (MIT, v0.5.0 2025-03-13), `ic-websocket-cdk-rs` (MIT, v0.4.2 2025-06-17), Motoko CDK on mops | [sdk-js](https://github.com/omnia-network/ic-websocket-sdk-js), [cdk-rs](https://github.com/omnia-network/ic-websocket-cdk-rs) |

**Architecture — it polls; there is no on-chain subscription primitive.** All from the [README](https://github.com/omnia-network/ic-websocket-gateway):

- Client side: the JS SDK creates a **signed IC envelope** and sends it over the WebSocket. The gateway forwards it to the IC's `/canister/<canister_id>/call` endpoint, *"This way, the WS Gateway is transparent to the canister, which receives the request as if sent directly by the client which signed it."* So the IC (not the gateway) authenticates the client's `sender`/`sender_pubkey`/`sender_sig`.
- Canister→client: *"the WS Gateway polls the canister by sending periodic queries to the `ws_get_messages` method."* The canister CDK pushes outgoing messages into a FIFO queue and, in the same update, certifies their hashes; the query returns the messages **plus a certificate** proving they came from the canister.
- Poll interval: **`--polling-interval`, default 100 ms** (configurable).
- The gateway multiplexes: it *"can provide a WebSocket interface for many different dapps at the same time"* and clients for the same canister share the gateway's polling queue.
- Ordering/anti-tamper: sequence numbers (ordering), timestamps (prevents silent delaying), and **message acknowledgement** from the CDK (prevents silent blocking). Keep-alive messages let the canister detect dead clients.
- Practical notes: the gateway has its own principal (`Gateway Agent principal` printed at startup), TLS flags (`--tls-certificate-pem-path`, `--tls-certificate-key-pem-path`), Prometheus/OpenTelemetry telemetry, Docker/Docker Compose deployment, and a load-test suite using Artillery.

**Can the gateway operator alter messages?**
- **[VENDOR] Integrity: no, not undetected.** *"the WS Gateway cannot tamper the content of the messages"* — server→client messages are certified; client→server messages are signed by the client identity. Reordering, delaying and blocking are also claimed to be detectable through sequence numbers, timestamps and acknowledgements. ([README](https://github.com/omnia-network/ic-websocket-gateway); the same claim is made by the DFINITY PoC: *"messages sent by the canister are certified; messages sent by the client signed by it"* — [ic-websocket-poc](https://github.com/dfinity/ic-websocket-poc))
- **[VENDOR] Confidentiality: no — it can read everything.** The README carries a bolded caveat: *"IMPORTANT CAVEAT: NO ENCRYPTION! … in principle the messages could be seen by others on the gateway and canister side. This will be solved in the next version of IC WebSocket using VetKeys."* So as of the current README the gateway is a **trusted reader** and a **trusted liveness dependency**: it cannot forge, but it can read, and it can stall traffic (detectably).
- **[VENDOR] A malicious gateway is also indistinguishable to the canister in one respect:** the maintainers acknowledge the gateway relays client messages *"mostly in a fire-and-forget way"* and that the design keeps the protocol close to native WebSocket. ([forum reply, 2024-07-27](https://forum.dfinity.org/t/websockets-on-the-ic-a-proof-of-concept/20836/14))

**Cost model.** I could **not verify any published price list** — `icws.io` does not resolve, so no pricing page was reachable. What is verified:
- The gateway is **MIT-licensed and self-hostable at no licence cost**; the maintainers also *"host a fully managed version"* of it, and TLS is enabled on the production gateway at `wss://gateway.icws.io`. ([README](https://github.com/omnia-network/ic-websocket-gateway); [forum, 2024-07-27](https://forum.dfinity.org/t/websockets-on-the-ic-a-proof-of-concept/20836/14); [stable release post](https://forum.dfinity.org/t/ic-websocket-stable-release/23872))
- The gateway is an **off-chain server**, so its own running cost is hosting cost, not IC cycles. The IC-side cost of the pattern is the cost of the canister's query calls (free — see §4) plus update calls for client→canister messages, plus storage for the message queue and certified map. The DFINITY PoC adds the detail that queued messages and their certified-map entries are pruned only if older than five minutes. ([ic-websocket-poc](https://github.com/dfinity/ic-websocket-poc))
- **[U] UNVERIFIED:** whether Omnia charges for the managed gateway, and any rate/quota tiers.

**Maintenance status.** Actively but slowly maintained: gateway last release Feb 2025, last commit Jun 2025, Rust CDK v0.4.2 (Jun 2025), JS SDK v0.5.0 (Mar 2025); the community thread's last activity is Aug 2025. Repo is not archived and has open issues. ([GitHub API](https://github.com/omnia-network/ic-websocket-gateway); [stable-release thread](https://forum.dfinity.org/t/ic-websocket-stable-release/23872))

### Other OSS projects adding WebSocket / push semantics to ICP

| Project | License | What it is | Trust assumption introduced |
|---|---|---|---|
| [dfinity/ic-websocket-poc](https://github.com/dfinity/ic-websocket-poc) | Apache-2.0 | DFINITY's original PoC: gateway + polling, `ws_register`/`ws_open`/`ws_get_messages`/`ws_message`/`ws_close`, certified messages, sequence numbers, timestamps. **Archived** (read-only), pushed 2025-08-28. Self-described as *"very rudimentary"*, not production-ready, with TODO items for SSL, DDoS hardening, heartbeats and auth expiry. | Same as Omnia: gateway trustworthy for integrity (certified/signed), fully trusted for confidentiality (explicit "NO ENCRYPTION" caveat) and for liveness. |
| [omnia-network/ic_websocket_example](https://github.com/omnia-network/ic_websocket_example), [Motoko chat/ping-pong examples](https://github.com/iamenochchirima/ic-websockets-chat-mo) | (examples) | Reference apps over the Omnia stack | Inherits Omnia's assumptions. |
| [aliscie/ic-websocket-gateway](https://github.com/aliscie/ic-websocket-gateway) | MIT | A **fork** of the Omnia gateway (last pushed 2024-03-19); not an independent implementation. | Same. |
| [ic4j/ic4j-websocket](https://github.com/ic4j/ic4j-websocket) | (client library) | Java client for the IC WebSocket protocol. | Same (client side). |
| [dfinity/ic-bn-logs](https://github.com/dfinity/ic-bn-logs) | Apache-2.0 | WebSocket client for **API boundary node access logs**. Officially the only WebSocket on ICP infrastructure, but it is **not** canister ingress and carries no push semantics for app data. | Trust in the boundary node log feed (not certified). |
| No OSS SSE / long-poll gateway found | — | **[U] UNVERIFIED / not found:** the only community discussion of Server-Sent Events was answered with *"the closest thing that I'm aware of"* being IC WebSocket. | — |

**Cross-cutting trust summary.** Every WebSocket option for canister data on ICP today is a **proxy that polls on your behalf**. In all of them the operator:
- **cannot** forge or silently reorder/alter messages (certification + signatures + sequence numbers),
- **can** read all payloads (no encryption; VetKeys integration is announced as future work, not shipped per the current README),
- **can** delay or drop messages, in a way that is detectable but not preventable, and
- is a **hard availability dependency** for realtime delivery.

---

## 4. Cost and limits of serving a static frontend from an asset canister

All costs from the [Cycle costs reference](https://docs.internetcomputer.org/references/cycle-costs/); all limits from the [Resource limits reference](https://docs.internetcomputer.org/references/resource-limits/). Unit prices verified against the raw doc source and against the IC's own subnet config constants (`ingress_message_reception_fee: 1_200_000`, `ingress_byte_reception_fee: 2_000` in [`rs/config/src/subnet_config.rs`](https://github.com/dfinity/ic/blob/master/rs/config/src/subnet_config.rs)).

**Cycle unit and subnet-size assumption.** **1 trillion cycles = 1 XDR**; the docs convert with **1 XDR = $1.366430**. Base cost tables **assume a 13-node application subnet**; a **34-node fiduciary subnet scales as `34 × (cost / 13)`**, i.e. **≈2.6×**. ([Cycle costs, "Replication factors"](https://docs.internetcomputer.org/references/cycle-costs/); [Subnet types](https://docs.internetcomputer.org/references/subnet-types/) — application subnets 13 nodes, multiplier 1×; fiduciary `pzp6e`, multiplier proportional to node count, "34 nodes → ~2.6×".)

### Storage: 1 GiB for a month

| Subnet | Cycles per GiB per second | Cycles per GiB per **30 days** | ~USD |
|---|---|---|---|
| 13-node | 127,000 | **~329 billion** | ~$0.45 |
| 34-node | 332,153 | **~861 billion** | ~$1.18 |

([Cycle costs, cost table](https://docs.internetcomputer.org/references/cycle-costs/). Check: 127,000 × 2,592,000 s = 329,184,000,000; 332,153 × 2,592,000 = 861,180,576,000.)

Also relevant: **storage reservation**. Below **750 GiB** subnet usage the reservation per byte is 0; above it, reservation scales linearly up to 10 years of payments at subnet capacity (2 TiB). Canisters can opt out with `reserved_cycles_limit = 0`, but *"opted-out canisters cannot allocate new memory when subnet usage exceeds 750 GiB."* ([Cycle costs, "Storage reservation"](https://docs.internetcomputer.org/references/cycle-costs/); [DoS prevention](https://docs.internetcomputer.org/guides/security/dos-prevention/))

### Serving a query / serving an asset: does it cost cycles?

**No — query calls are free.**

- Cost table row: *"Query call | Query information from a canister | N/A | **Free** | Free | Free | Free"* — free on both 13-node and 34-node subnets. ([Cycle costs](https://docs.internetcomputer.org/references/cycle-costs/))
- *"Query calls are free: they run on a single node, do not go through consensus, and are not charged."* ([Concepts: Cycles](https://docs.internetcomputer.org/concepts/cycles/))
- Because the HTTP gateway fetches assets via `http_request` **query** calls (plus streaming query calls for large files), **serving a static frontend costs no cycles at all**. ([HTTP gateway protocol spec](https://docs.internetcomputer.org/references/http-gateway-protocol-spec/))

What *does* cost cycles for a frontend canister:
1. **Storage** for the assets, the certification tree, and (on the asset canister) the compressed variants — the 127,000 cycles/GiB/s above. The static-site canister stores text assets in three forms (identity, gzip, Brotli), keeping a compressed copy only if smaller. ([Under the hood](https://docs.internetcomputer.org/guides/frontends/static-site/how-it-works/))
2. **Ingress message reception** for uploads/deploys (these are update calls): **1,200,000 cycles per ingress message** + **2,000 cycles per byte** on a 13-node subnet (34-node: 3,138,461 and 5,230). ([Cycle costs](https://docs.internetcomputer.org/references/cycle-costs/))
3. **Update message execution**: 5,000,000 base + 1 cycle per Wasm instruction on a 13-node subnet. ([Cycle costs](https://docs.internetcomputer.org/references/cycle-costs/))
4. **Canister creation**: 500,000,000,000 cycles (13-node) / 1,307,692,307,692 (34-node). ([Cycle costs](https://docs.internetcomputer.org/references/cycle-costs/))

*(Verified that query calls are the only "Free" row; the cost table has no per-request entry for asset serving.)*

### Per-request / per-response / asset-size limits

| Limit | Value | Subnet assumption |
|---|---|---|
| Max ingress message payload | **2 MiB** (3.5 MiB on the NNS subnet) | protocol-wide |
| Max response size, replicated execution (update calls) | **2 MiB** | protocol-wide |
| Max response size, non-replicated execution (**query calls**) | **3 MiB** | protocol-wide |
| Max cross-subnet inter-canister payload | 2 MiB | — |
| Max same-subnet inter-canister request payload | 10 MiB | — |
| Stable memory per canister | **500 GiB** | — |
| Wasm heap per canister | 4 GiB (wasm32) / 6 GiB (wasm64) | — |
| Stable memory read/written per query call | 1 GiB each | — |
| Wasm total size per canister | 100 MiB | — |
| Subnet total memory capacity | 2 TiB | — |

([Resource limits](https://docs.internetcomputer.org/references/resource-limits/))

Asset-canister-specific:
- *"**Storage limits.** The asset canister can hold well over 4 GiB in stable memory, but **individual uploads are limited by the 2 MB ingress message size** (the JS SDK handles chunking automatically for larger files)."* Deploying a file >1.9 MB is chunked automatically (`AssetManager.store`). ([Asset canister (legacy)](https://docs.internetcomputer.org/guides/frontends/asset-canister/))
- **Large responses are chunked**: *"A single canister response has a bounded message size, so a large file can't be returned in one shot. The canister splits it into chunks and serves each chunk as a certified `206 Partial Content` response carrying a `Content-Range`."* The gateway reassembles them (or returns the requested range). Every chunk is certified. ([Under the hood](https://docs.internetcomputer.org/guides/frontends/static-site/how-it-works/); [HTTP gateway protocol spec, "Response Body Streaming"](https://docs.internetcomputer.org/references/http-gateway-protocol-spec/))
- **Dynamic routes are not possible**: *"No dynamic URL routing at the server level"*; SPA routing is aliasing to `index.html` (asset canister) or a `_redirects` rule (certified-assets). **No server-side rendering.** ([Asset canister (legacy)](https://docs.internetcomputer.org/guides/frontends/asset-canister/))

### Ingress / query rate limits per canister

**Not documented as a per-canister requests-per-second figure.** What the official docs do specify:

| Documented limit | Value | Source |
|---|---|---|
| Messages included per block | up to **1,000** | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Block production rate | **1–3 blocks per second**, depending on subnet load and node count | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Message queue limit between a canister pair | **500** | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Query execution threads per replica node | **4** | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Query execution threads per canister | **2** | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Update execution threads per subnet / per canister | 4 / **1** | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Instructions per query call | 5 billion | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Instructions per update call / heartbeat / timer | 40 billion | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| Ingress message expiry | set by the agent, **max 5 minutes** | [Resource limits](https://docs.internetcomputer.org/references/resource-limits/) |
| `read_state` paths per request | at most 1000 paths, each ≤127 blobs | [HTTP interface spec](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/) |

The practical rate ceilings are therefore **concurrency- and block-bound, not a published per-canister RPS number**. The only explicit RPS figures I found are boundary-node-level and come from a **[STAFF]** forum answer, not docs:

- *"In general, there are no limits on query calls. However, there are some global limits on the number of requests a single client can make in given amount of time. If a client exceeds that limit, it will be banned for a couple of minutes from all access to anything hosted on the Internet Computer. Specifically, a client is banned for 10mins if it exceeds **1k rps**."* ([DFINITY staff, 2025-06-30](https://forum.dfinity.org/t/question-about-query-call-limits-to-icp-canisters-from-a-server/51457/2))
- *"Each BN only allows for **1k update calls per subnet and second**. If you run into this, you will get **429** status codes… Each BN shed load if the system utilization reaches a certain threshold… Excessive amounts of requests (**> 1k rps**) will lead to a temporary ban of a few minutes."* ([DFINITY staff, 2025-03-14](https://forum.dfinity.org/t/rate-limit-on-data-construction-apis/42266/2))

### Serving from a non-gateway path (for completeness)

Query responses are not certified, and *"There is no particular order guarantee for ingress messages submitted via the HTTPS interface."* If you need a certified read without a gateway, you can submit a query method as a `call` request — that is an update call and therefore **is** charged cycles. ([HTTP interface spec](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/); [Resource limits](https://docs.internetcomputer.org/references/resource-limits/))

---

## 5. Polling from a browser to a canister at ~1 Hz

### Cost

**A 1 Hz browser poll using query calls costs the canister zero cycles.** 3,600 queries/hour × 0 cycles = 0. This follows directly from the two official statements that query calls are free and not charged ([Cycle costs](https://docs.internetcomputer.org/references/cycle-costs/); [Concepts: Cycles](https://docs.internetcomputer.org/concepts/cycles/)).

**If the poll is an update call instead, it is expensive.** Derived from the official unit costs on a 13-node subnet, a minimal update poll costs ≈ 6,200,000 cycles per call (5,000,000 update base + 1,200,000 ingress reception), *before* instruction fees (1 cycle each) and byte fees (2,000/byte):

- at 1 Hz: ≈ **22.3 billion cycles/hour** ≈ **0.0223 XDR/hour** ≈ **$0.031/hour** ≈ **$0.73/day** on a 13-node subnet.
- on a 34-node subnet the same call is ≈ 16.2 million cycles → ≈ **$1.9/day**.

*(This is my arithmetic from the published unit prices, not a figure printed in the docs — treat it as an order-of-magnitude estimate, and note that a real method's instruction count can dominate.)*

Note also: a browser that polls with updates must poll `read_state` after each call to learn the result, adding more ingress messages. ([HTTP interface spec](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/))

### Rate limits

- **No documented per-canister RPS limit for query calls.** A **[STAFF]** answer states plainly that there are no query-call limits, with the caveat of a global per-client cap: **> ~1k rps leads to a ~10-minute ban** from all IC-hosted access. ([DFINITY staff](https://forum.dfinity.org/t/question-about-query-call-limits-to-icp-canisters-from-a-server/51457/2))
- At **1 query/second, a single client is ~3 orders of magnitude below that threshold**, so from the boundary-node perspective 1 Hz polling is not rate-limited.
- Boundary nodes also **shed load with HTTP 429** when system utilization crosses a threshold, and cap **1k update calls per subnet per second per BN**. ([DFINITY staff](https://forum.dfinity.org/t/rate-limit-on-data-construction-apis/42266/2))
- **Protocol-level** ceilings that bound polling rather than forbid it: **2 query execution threads per canister** (4 per replica node), 5 billion instructions per query, 1 GiB stable-memory read per query, 3 MiB max query response. ([Resource limits](https://docs.internetcomputer.org/references/resource-limits/))
- API boundary nodes **cache some query responses** to reduce latency and load, which in practice can absorb some polling traffic. ([Edge infrastructure](https://docs.internetcomputer.org/concepts/edge-infrastructure/))

### Do updates have stricter limits than queries?

**Yes, materially.** Verified differences:

1. **Charging:** queries are free; update messages carry a base fee plus a per-byte variable cost, and *"ingress messages (user to canister) are charged to the receiving canister."* ([Concepts: Cycles](https://docs.internetcomputer.org/concepts/cycles/); [Cycle costs](https://docs.internetcomputer.org/references/cycle-costs/))
2. **Signed ingress:** update calls need an authenticated envelope (`sender_pubkey`/`sender_sig`), so they need an identity and (for non-anonymous) signing; queries can be anonymous. ([HTTP interface spec](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/))
3. **Explicit BN cap:** **1k update calls per subnet per second per boundary node** → HTTP 429. There is no published equivalent cap for queries. ([DFINITY staff](https://forum.dfinity.org/t/rate-limit-on-data-construction-apis/42266/2))
4. **Instruction budget:** 40 billion per update call vs 5 billion per query call. ([Resource limits](https://docs.internetcomputer.org/references/resource-limits/))
5. **Execution threads:** 1 update thread per canister vs 2 query threads per canister; 4 update threads per subnet vs 4 query threads per replica node. ([Resource limits](https://docs.internetcomputer.org/references/resource-limits/))
6. **Throughput coupling:** update calls are limited by subnet block production (1–3 blocks/s, ≤1,000 messages/block) and go through consensus; queries do not. ([Resource limits](https://docs.internetcomputer.org/references/resource-limits/))
7. **Certificate semantics:** query responses are not certified unless the canister certifies them; update responses are certified. ([Response certification](https://docs.internetcomputer.org/guides/frontends/certification/); [HTTP interface spec](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/))

### Practical implications for a 1 Hz UI poll

- **Cost is a non-issue** for query polling; the real costs are **latency** (single-replica, cache-dependent, no ordering guarantee) and **correctness** (uncertified responses unless you certify).
- **A 1 Hz poll per browser tab does not approach any documented rate limit**, but N concurrent users each polling at 1 Hz multiply the load on one canister hitting a **2-thread query limit** — that is the first ceiling an app is likely to hit, not the boundary-node RPS cap.
- The documented alternative that reduces per-client polling is the IC WebSocket gateway, which still polls the canister but **multiplexes many clients behind one poller** (default interval **100 ms**, configurable) and shifts the polling and delivery into a server you must trust for confidentiality and availability. ([IC WebSocket gateway README](https://github.com/omnia-network/ic-websocket-gateway); the same trade-off was raised in the original roadmap thread — *"Does the WS gateway poll only once from the canister each X for all users or it polls separately for each user?"* — [roadmap thread, post 4](https://forum.dfinity.org/t/ic-websocket-roadmap/21503))

---

## Explicitly unverified / not found

1. **No official DFINITY or NNS roadmap statement dated 2024–2026** confirming or scheduling native WebSocket ingress for canisters. Only the 2023-era statements above were found.
2. **No published pricing for Omnia's managed IC WebSocket gateway.** `icws.io` did not resolve; the forum posts and README confirm a managed gateway exists at `wss://gateway.icws.io` but state no price.
3. **No OCR-verified content** for the `internetcomputer.org/blog/features/websockets-poc` post — that URL cross-origin-redirects to Medium and was not followed. The equivalent content was taken from the DFINITY forum announcement instead.
4. **No documented per-canister ingress or query RPS limit in official docs.** The 1k rps / 1k update-calls-per-subnet-per-second figures are forum statements by a DFINITY employee, not documentation.
5. **No OSS Server-Sent-Events or long-poll push gateway for ICP canisters was found.** The only community answer to that question points back to IC WebSocket.
6. **VetKeys integration into IC WebSocket is announced as future work** in the gateway README, and I found no release note showing it shipped; therefore gateway-side read access to messages should be assumed to still exist.
7. **`dfinity/ic-http-gateway` returns 404** from the GitHub API; I could not retrieve a license or status for that specific older repo.
