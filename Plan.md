# private-perp implementation plan

> Historical design record. Dates, decisions, estimates, and verification status refer to the original record. See [implementation status](docs/implementation-status.md) and [single-canister architecture](docs/phase-3/single-canister.md) for later changes.


- Version: v0.9 (UI base implementation, not yet approved for production)
- Last update: 2026-09-18
- Scope: Requirements definition, permission model, architecture, roadmap, unresolved issues
- Not applicable: Detailed design (data model, Candid IF, screen specifications), implementation task decomposition, labor estimate

This document serves as a record of decision-making. If you change your policy, update the "0.3 Decision Record" and leave a reason for the change.

---

## 0. The position of this document

### 0.1 Purpose

"What to make," "What not to make," and "Who can do what" will be fixed before implementation. confidential canister will maintain the policy of holding customer funds and governing by SNS DAO. In v0.7, the effect of reducing the public links to everyday wallets and HL trading accounts will be treated as a qualifying condition alongside fund security. We will maintain HL standard fill, margin, and liquidation as much as possible.

1. Customer funds and internal balance and entry/withdrawal handling tables are managed in the confidential fund layer and separated from the DAO operation funds.
2. fund layer can execute authorized deposits/withdrawals and fund transfers. It does not give withdrawal permissions to trading agents.
3. Without leaving the individual controller rights of the operator, we will govern the SNS as a governance entity. However, because the freedom of upgrade rights may lead to fund transfers and disclosure of confidential information, the restrictions on change rights and the exit/recovery pathways will be determined before the official launch.

This is a planned architecture decision and not an approval for SNS launch, token issuance, live deposit acceptance, and deployment. It does not guarantee security, self-custody, or anonymity based solely on SNS management and tECDSA.

### 0.2 readers

Implementation, review, legal, operations. What is necessary for legal and operations is Chapter 3 (authority model), 6.5 (eligibility), Chapter 10 (risk), and Chapter 14 (pending matters).

### 0.3 Decision record

2026-09-28: The public test version will use a single Canister and the app administrator specified when installing. This is a change to the arrangement of five Canisters and SNS/guard, and compatibility with the previous configuration will not be provided. For details and changes in warranty, refer to [Single Canisterization](docs/phase-3/single-canister.md). This does not mean the implementation of the following operational governance policy.

| # | the point | Decision | Decision date | Influence |
|---|---|---|---|---|
| D1 | Creating a Hyperliquid account for trading only | Authenticate with MetaMask EOA and funds_vault manages user-specific independent HL master keys with tECDSA | 2026-09-18 | Do not provide the trading account master key to the connected wallet. Recovery and withdrawal follow the 16.1 procedure. |
| D2 | The level of eligibility verification | Geo/VPN blocking + non-US citizen self-declaration + sanctions/wallet screening. KYC is not conducted. | 2026-09-18 | Require a structure that can add KYC later (6.5.4) |
| D3 | Target market | All perps. However, activation is gradually released according to the allowlist. | 2026-09-18 | Signature and state machines can be reused without being dependent on the asset. What increases is the range of effects when the invariant conditions of the asset index are breached, (a) risk parameters dependent on liquidity, (b) operational work for the delisting. Refer to 8.5.4, 6.3.1. |
| D4 | Order type | Market / Limit / Cancel / Cancel All / Close、SL / TP（reduce-only） | 2026-09-18 | It is necessary to design the differentiation of grouping and the linkage of positions. |
| D5 | Revenue model | Builder fee | 2026-09-18 | Set the fee when placing an order. Revenue recognition will be implemented after Phase 3. |
| D6 | Technology stack | Rust canister + TanStack Start/React/TypeScript. Delivered with Workers + Static Assets. | 2026-09-18 | Signature and funds will be retained in ICP. Next.js/Preact is not adopted. ADR-0005/0006 |
| D7 | Placement destination | Deployed on Confidential Subnet from the early development stages | 2026-09-18 | Availability of development environments is the most important prerequisite (8.3, Chapter 14) |
| D8 | Operator | Undetermined | 2026-09-18 | Described as an entity-independent constraint. |
| D13 | Privacy goals | Reduce the number of public links to regular wallets and HL trading accounts and positions. We do not guarantee complete anonymity, but we do not consider the configuration that can be easily supported from the amount and time as satisfactory. | 2026-09-18 | First set the observer, evaluation conditions and pass thresholds, and directly measure improvements to the deposit method from the initial spike (6.4.1). |
| D14 | Compatibility with HL standard | Design the standard fill, margin, and settlement to be the priority policy for user-specific HL accounts. | 2026-09-18 | Do not confuse the common funding custody account with the HL account that aggregates all positions. Do not consolidate into a single HL account. |
| D15 | confidential fund layer | Canister adopts the B proposal of holding customer funds and managing internal balance, deposit and withdrawal, and trading account operations in a non-public manner. | 2026-09-18 | Withdrawal restrictions for Agent-only, lack of master key, and direct recovery guarantee are withdrawn from the immutable conditions of the entire service. Include the funds ledger and withdrawal/recovery in the MVP. |
| D16 | Fund management governance | As the governance entity of SNS DAO, restrict changes to the funding relationships Canister through immutable control_guard. | 2026-09-18 | Adopt a 7-day execution delay and bypass prohibition (16.3). Audit the remaining risks of guard's own malfunction, availability, and confidentiality. |
| D17 | The verification conditions of the funding custody proposal | Maintain the development policy of B and make both funding safety and privacy improvement conditions for the actual transition. | 2026-09-18 | Verify the reverse process of deposit, HL allocation, trading, recovery, and withdrawal in Phase 1. If not met, redesign it, and do not consider it compliant if automatic switching to Agent-only or weakening of claims is not implemented. |

D7 expands the scope of verification. It defines testnet verification and production shutdown in 8.3.3 in case Confidential Subnet cannot be ensured. The period including D15/D16 should be re-estimated (Chapter 11).

Regarding D3, even if the number of assets increases, the signature and state machine code will not increase. The assets can only be one integer (asset index) within the action. The increase comes from the range of effects when the invariant condition of the asset index is broken (8.5.4), the liquidity parameters that cannot be obtained from Hyperliquid's meta (6.3.1), and the operational work of the delisting. Therefore, the allowlist is treated as the core mechanism that limits the range of effects that a single defect can have, rather than a formal phased unlocking.

### 0.4 Unresolved matters

It has been consolidated into Chapter 14. The implementation choices were finalized in Chapter 16. Even if the live conditions are not fully met, local implementation and testnet verification can be initiated using synthetic data. We do not finalize external specifications, experimental results, or legal judgments based on speculation. Live funding, SNS launch, and paid service contracts require separate approval.

### 0.5 Determination of implementation policy

Record D9 (signing route), D10 (trading_core placement), D11 (real-time route), and D12 (persistence) in `Implementation.md` chapter 0. Record implementation decisions there, and return authority-model or invariant changes to this document (3.2).

`Implementation.md` v0.5 supports the B plan of this project. It retains the existing order status, fencing, and signature reconciliation, and aligns the funds ledger, master key, authentication, communication, and change permissions. The values in Chapter 16 are the design default values and do not mean that they are implemented or audited. The scope of implementation for the UI foundation and composite demo is refer to docs/implementation-status.md.

---

## 1. Product definition

### 1.1 Definition in one sentence

A service that allows users to use HL standard perpetual futures trading while reducing the public links to their regular wallets and Hyperliquid trading accounts, and depositing funds into confidential canister, which is governed by SNS DAO, with the exclusion of US citizens and restricted regions.

We entrust orders, fills, margin, and settlements to Hyperliquid standards as much as possible. Since Canister manages customer funds, we do not assume that users can independently manage their funds or directly withdraw funds during service suspension. We distinguish between technical custody and transfer permissions and legal custody classification, and do not determine the entire service as "non-custody."

### 1.2 Problems to solve

- Public wallets and positions are linked, allowing third parties to track order direction, size, liquidation price, and PnL.
- For automatic trading, you need to pass your private key to the bot or the operational server.
- Existing bot SaaS allows businesses to custody their API keys, making them a single point of failure and a single point of trust for the business.
- If you deposit to CEX, you can get privacy, but you will have to hand over asset management to the exchange.

This service is designed to keep the user ID, order, and account correspondence confidential, and to separate the funding pathways for dedicated trading accounts. Even if you do not disclose the correspondence table to the public API, the links generated directly from transfers or public approvals will not be resolved.

### 1.3 Value provided and value not provided

Aim at.

- Separation of existing public wallets and trading-only accounts.
- Do not expose the user-friendly table and order details as a Canister API.
- Do not disclose unique conditions and strategies before sending to HL. Do not claim that even orders, limit orders, and fills sent to HL are kept secret.
- The single entity should not hold the entire private key of the Canister Agent. This does not mean that the asset withdrawal rights do not exist.

Do not provide.

- Complete anonymity. Positions, balance, and fills of trading-only accounts on Hyperliquid are public information.
- Complete resistance to correlation analysis based on amount and time. However, this does not mean that it is a reason to omit correlation countermeasures. The configuration that can easily accommodate public withdrawals and HL trading accounts is classified as D13 not met, and improvements from the direct deposit method are evaluated in 6.4.1.
- Confidentiality from Hyperliquid itself. The operation of Hyperliquid can be identified by the account.

External expressions should be aligned with the verified protection scope. The term "confidential trading" does not imply the secrecy of withdrawals alone, nor does it indicate "complete anonymity" or "untraceable." This rule applies to all UI, terms of service, and marketing materials.

### 1.4 Confidentiality of funds and separation of trading accounts

The configuration comparison and decision are as follows: adopt B (D15). Do not start the actual deposit before the detailed confirmation of funding, key, and recovery design.

| composition | Funding route and public link | HL's trading processing | Power and dependence |
|---|---|---|---|
| A: Old Agent-only proposal (not adopted) | Deposit directly from the user's dedicated HL account. The transfer graph link remains. | Standard processing for user-specific accounts | master key is for users. ICP is only for tradingAgent. |
| B: confidential fund layer + user-specific HL account (hire) | Make the handling of internal balance and withdrawal transactions confidential. External amounts and times may remain available. | Standard processing for user-specific accounts | vault manages the master key, and changes from SNS are passed through guard (Chapter 16) |
| C: Single HL account + internal position | Internal management of allocation of funds and positions | Internal margin, profit and loss allocation and settlement are required. | All users' funds and trading risks are concentrated. This plan will not be adopted. |

Custodying assets in the common Treasury does not mean consolidating everyone's HL positions into a single account. B's private ledger is the funds balance ledger, and it is separate from C's unique position and clearing ledger.

Fact verified according to NEAR's official specifications (2026-09-18):

- Confidential Intents uses the FAR of the public NEAR Treasury and the private NEAR fork. When depositing, IMT is issued on FAR with 1:1 backing, and when withdrawing, it is burned to release the backing.
- FAR balance and swap settlement are not publicly disclosed. near.com is explicitly noted as an example of an integration that retains confidential balance.
- The HL integration of 1Click is to specify the user's HL address as the recipient and send USDC to perpsbalance.

From this combination, we can infer a configuration close to B, but the account allocation, HL master key, and withdrawal permissions that near.com's perps actually adopt are not confirmed. We do not make any definitive statements about "NEAR using a single HL account" or "because there is a fund layer, the operating company can freely withdraw funds." The boundary between the materials and the inferences will be tracked in the NEAR official materials in Chapter 15.

The hiring of B does not mean achieving the correlation resistance of the amount and time. If the structure is such that only the same amount is sent to the dedicated HL account immediately after deposit, and the response is easy to implement based on public observation, it will not be considered a pass. We will measure the privacy improvement that becomes a reason for accepting the risk of funding custody (6.4.1, U23/U29). Making the internal funds allocation of multiple users non‑public and aggregating the HL positions are separate.

NEAR is treated as a reference structure. It is not used as the basis for approval of this plan because of the existence of deposits into the protocol and confidential balance, the management of the master key of near.com perps, the operator's mobility rights, and the resistance to amount and time correlation.

---

### 1.5 Structural constraints and implementation gates

The decentralization of signature keys, the confidentiality of fund pathways, and the ownership of HL accounts are separate issues. A does not hide the direct deposit public link and cannot achieve D13 alone. B adopts the 16.1–16.2 funding pathways and master key management to verify security and correlation resistance in Phase 1.

When a user owns a trading account master, it requires the creation, custody, recovery, and signature UX of a key separate from the connection wallet. If Canister manages the master, it cannot reuse the Agent-only "withdrawal not allowed" and "user can exit alone" features. Permissions are evaluated, including code changes by the controller.

HL will delegate SL/TP, margin, and liquidation to HL. ICP implements a ledger of funds held, but does not duplicate the unique position liquidation engine. Order acceptance, HL processing, and fill are separated, and the detailed order state model follows Chapters 4 and 5 of Implementation. Currently, it only reaches the synthetic UI foundation, and the ICP funds state machine and real-account connection are unimplemented.

## 2. a term

Because the polysemy of "account" can cause design errors, use the following words to distinguish them.

| a term | a definition | Management entity |
|---|---|---|
| User ID | Private random ID. Supports short-lived sessions Principal with connection EOA and authentication | funds_vault manages authentication support |
| trading account | Independent Hyperliquid accounts allocated according to the user. HL standard margin and liquidation units | master is funds_vault, Agent is trading_core |
| Agent Wallet | trading_core is a trading-specific agent that operates with threshold ECDSA. It does not grant withdrawal or transfer permissions. | The key is ICP, and the approval entity follows the design of HL master permissions. |
| service | The overall foundation related to frontend, trading_core, funds_vault, policy_registry | Governance is SNS, and daily operations are limited-access management. |
| cloid | Hyperliquid client-issued order ID (128bit) for preventing duplicates and reconciliation | trading_core generated |
| eligibility token | Short-lived eligibility tokens with signatures issued by external compliance gateways | Gateway |
| Connection wallet | A regular wallet that connects and authenticates using MetaMask, etc. It does not need to be the same address as the trading account. | a user |
| confidential fund layer | A layer that holds customer funds and manages balance and deposits and withdrawals in a non-publicly managed manner. Separate from position management on HL. | funds_vault, governance is on SNS |
| Customer funds | A custody asset that handles debts and holdings for users. It is not an operational budget that the DAO can freely use. | Managed with authorized funding logic |
| DAO operation funds | Assets such as SNS Treasury, which are used for cycles, development fees, etc. | Managed as the operating budget for SNS |

This service receives customer funds. If you write "account" alone in this document, it refers to a trading account. The MetaMask connection is the gateway for authentication and deposit/withdrawal, and it does not mean that the user holds the HL master key.

---

## 3. Authority model

This chapter is the goal privilege model of B. tECDSA, TEE, and SNS respectively take on the governance of key protection, confidentiality, and change privileges, but do not guarantee the security of funds automatically. The possibility of changing the privilege table through upgrades will be handled separately in 8.3.4.

### 3.1 Permission table

| operation | a user | trading_core / Agent | funds_vault | Operation / SNS |
|---|---|---|---|---|
| Order/cancellation/payment | authorization/request | Verify and enforce | Not responsible for the principle | Arbitration trading is not allowed by the operation. |
| Deposit recording and allocation to HL | deposit and allocation are authorization | No withdrawal rights | Manage deposit confirmation and balance/allocation | Voluntary relocation by the operation is not allowed. |
| withdrawal | The person authorizing the recipient and amount | Prohibited | Verify balance and restraint and enforce it | No DAO voting required for normal withdrawal |
| Transfer of funds to any third party | Only the withdrawal of the user's balance | Prohibited | Not possible without valid authorization | Repurchasing as a DAO budget is prohibited. |
| Agent approval/decline | Request to stop/unsubscribe | Stop new signature | Execute and reconcile with master signing | HL direct uninstallation is not allowed by users. |
| New order suspension and risk limit reduction | Stop your trading | Force restrictions | Limit new allocation | Possible with limited operating rights |
| Withdrawal destination and change of balance | owner authorization is required | Prohibited | Only state transition with evidence | We do not provide an operation API for optional changes. |
| Changing the WASM controller | Verify and exit public information | Not allowed by self-judgment | Not allowed by self-judgment | SNS + guard. Upgrade is allowed for 7 days, controller change is prohibited (16.3) |

This table is usually the code's authority. Even if SNS can upgrade at its own discretion, funds and information leaks caused by malicious decisions of the DAO will remain.

### 3.2 Design invariable conditions

1. **Separation of funds and trading permissions.** trading_core only has a trading agent. funds_vault is responsible for fund transfers and does not publicly disclose any digestsigning API. It not only verifies the caller but also the purpose of the transfer, amount, destination, owner authorization, and balance.
2. **Separation of customer funds and operational funds.** Do not divert customer funds to cycles, development expenses, or DAO Treasury transfers. Fees are charged according to the agreed rules, and undecided funds are not treated as revenue.
3. **Consensual alignment of holdings and backing.** Double counting and duplicate withdrawals are prohibited, and the available amount, assets allocated to HL, margin restrictions, withdrawal reservations, and transfer periods are distinguished. Align the reconciliation date and valuation standards to ensure that real assets, HL holdings, user liabilities, fees, and profits and losses are consistent. Do not add any amounts that cannot be withdrawn to the balance.
4. **withdrawal owner authorization.** Restricts the user's withdrawal requests to the amount, recipient, chain, asset, nonce, and deadline. Typically, DAO voting is not required for withdrawals, and operations are designed to prevent the recipient from changing the address. Authorization is granted using an EOAsignature on 16.1.
5. **Reconciliation of uncertain external effects.** Before transfer, permanently retain the reservation and operation ID and do not consider response loss as untransfer. Do not release or resend the reservation without confirming that the external transfer has not been completed.
6. **Key and change permissions.** tECDSA keys are bound to Canister ID, derivation path, and key ID. Even malicious new code for the same Canister can be signed. We audit the upgrade, controller change, suspension, and deletion of SNS as permissions for funds and data.
7. **Suspension, withdrawal, and recovery.** Separate trading and withdrawal suspensions. We will not offer unconditional withdrawal if the integrity of the signature or balance is unknown. We will document the recovery entity, procedures, and dependencies during service suspension, and we will not promise that recovery can be performed independently by the user while remaining unverified.
8. **Impact of violations.** Even with agent violations, you can lose your margin through fraudulent trading. A violation of the fund layer or its change permissions can make all your held assets vulnerable. TEE does not prevent malicious regular code, and SNS does not prevent implementation bugs.

### 3.3 Permissions not granted to the operator

- Customers' voluntary withdrawal of funds, changes of withdrawal destination and transfers to other accounts without owner authorization.
- Complete private key acquisition, funds keysignature for arbitrary messages.
- Transfer of customer funds to the DAO operation budget.
- Orders exceeding the user's stated limit.
- Arbitrary interference with legitimate trading suspension and withdrawal requests.
- Viewing the plain text order content (in accordance with the policy of 8.3.2).

---

## 4. Target users

Target.

- Hyperliquid users who are not American.
- Users who do not want to connect existing wallets and positions on the blockchain.
- Users who want to use automatic orders and conditional orders without giving private keys to third parties.
- Users who handle relatively large positions at low frequency instead of high frequency trading.

Not applicable.

- Americans, sanctioned individuals, residents of restricted areas.
- HFT that requests execution in milliseconds.
- Users who seek complete anonymity.
- Users who must always be able to recover immediately on their own even while the service is suspended (this guarantee is not provided).

---

## 5. User flow (confidential fund layer + SNS management)

Use MetaMask EOA as the gateway for connection, authentication, and withdrawal approval. The master key for the trading-specific HL account is managed by funds_vault (16.1). Before depositing, explain the Canister custody, SNS change permissions, confidentiality scope, and recovery conditions in case of suspension.

### 5.1 Onboarding

1. Access the service. At this point, the region, VPN, and hosting IP are determined.
2. Authenticate session Principal with a one-time signature for MetaMask EOA (16.1).
3. Agree to the Terms of Use and to declare that you are not a non-US citizen.
4. Go through sanctions/wallet screening. If you pass, an eligibility token will be issued.
5. Link authentication in step 2 to the user ID and limit the withdrawal destination to the same EOA. Confirm that there is no compensation in case of EOA loss.
6. Assign the dedicated HL account and Agent to the confirmed key management method. The corresponding table will not be made public.
7. The entity with master permissions approves the Agent, and the service reconciles the validity. Direct user approval is not mandatory.
8. Explain that the user deposits USDC from their HL account to the shared reserve account (16.2). Explain that the transfer graph, amount, and time are publicly displayed.
9. The fund layer confirms the deposit and removes duplicates, and records it into the internal balance. It allocates it to the dedicated HL account within the range authorized by the user, and reconciles the arrival.

### 5.2 trading

1. The user creates an order.
2. The client encrypts the order payload and sends it to trading_core (8.3.2).
3. trading_core verifies authorization, limits, expiration dates, and eligibility tokens.
4. Permanentize the pending state and cloid.
5. Build an action in Hyperliquid format and generate an EIP-712 hash.
6. Sign with tECDSA.
7. Re-verify workergeneration, permissions, deadlines, policy, and risks, then permanently establish dispatching before POSTing to Hyperliquid via non-replicated HTTPS outcall.
8. POST responses do not confirm success.
9. Search and reconcile `/info` in cloid.
10. Save the reconciliation results of the action separately and the open / partially_filled / filled / cancelled / rejected / unknown status of each order.

### 5.3 trading suspension, withdrawal, membership cancellation

- Users can stop their new orders and automatic trading. Existing orders and positions will be cancelled and settled separately. Agent uninstallation is executed by funds_vault.
- The withdrawal request is authorized by the person in charge of the amount and destination, and funds_vault verifies the balance and margin restrictions and reserves it. If recovery from HL is required, it will be completed by reconciliation before transferring.
- Display them separately as "Request Received", "HL recovery in progress", "transferunknown outcome", and "Completed". Do not confirm withdrawable amount or completion time as unconfirmed.
- Resolve undecidedtransfer, debt, and positions before canceling membership. Do not delete the ledger required for balance and recovery solely by requesting cancellation.
- The alternative request path when UI stops, recovery when Canister stops, and exit procedure before SNS changes are determined in U24/U26. Alternative UI does not solve the problem of Canister itself stopping.

---

## 6. Function requirements

### 6.1 Onboarding

- Login with MetaMask EOA. Internet Identity, passkey, and smart contract wallet are not eligible for initial use.
- Agreement with the Terms of Use and version recording.
- Self-declaration of non-U.S. citizen status. Request re-declaration regularly.
- Region, VPN, hosting IP determination, sanctions screening, wallet screening.
- Verification of ownership of the connection wallet, linking to the user ID, and allocation of the dedicated HL account.
- Retrieve and display the user-specific Agent address.
- Agent approval through the adopted master permission management method and on-chain reconciliation after approval.
- Display of Agent expiration date and re-approval reminder (8.4.3).
- Explanation of the reason to separate a trading-only account.

### 6.2 trading

Corresponding order type (D4).

- Market、Limit、Cancel、Cancel All、Close Position。
- Stop Loss、Take Profit（reduce-only）。

The linking of positions (position TPSL) and individual orders (normal TPSL) is expressed by using grouping. Which one is set as default is determined during UI design (Chapter 14).

Target stock (D3).

- Architecture handles all perps. Signature and state machines are not dependent on the brand.
- Activation is done in a explicit allowlist. Do not open all perps at once.
- Addition to the allowlist is determined by liquidity criteria (6.3.1). Stocks that fall below the criteria will not be activated.
- The ticker metadata (szDecimals, maximum leverage, margin mode) are obtained from Hyperliquid's meta and used for order verification. Do not hardcode.
- The handling of the asset index follows the invariant conditions of 8.5.4.
- Has a kill-switch per brand unit.
- If a stock that has been activated has a liquidity criterion below, it will automatically stop new orders.

Information displayed on the screen.

- Available balance, current position, average acquisition price, liquidation price, Unrealized PnL, open orders, order history, fill history, Agent connection status and expiration date.

Market data can be directly obtained from the browser to the Hyperliquid public API. The order settlement status is only confirmed by the reconciliation result on the Canister side.

### 6.3 Risk management

- Order size limit, leverage limit, limit per asset.
- Maximum slippage, price deviation check, order validity period.
- Reject old orders and old nonce.
- Prevent duplicate sending of the same order (cloid).
- Safe payment by reduce-only.
- Rate limit (per user, per Principal).
- Nonce monotonicity and collision avoidance (8.5.3).
- Emergency cancel-only mode.
- Requests for trading suspension and Agent removal by users. We do not provide direct removal of HL by users who do not have master.
- dead-man's switch (new orders are stopped when no operation is performed for a certain period. The default is invalid, and the user can activate it).

#### 6.3.1 Release criteria for securities based on liquidity

Among the different risk parameters per stock, liquidity is the one that cannot be obtained from the meta of Hyperliquid. maxLeverage is that 228 of 234 stocks are below 10x, which is enough for a conservative upper limit. szDecimals can be obtained from the meta, and only 1 stock (ZEC) out of 178 live stocks had a nominal order unit value exceeding 10 USD. On the other hand, liquidity opens more than 5 digits.

Actual values as of 2026-09-18.

| an indicator | Minimum | Median | Maximum |
|---|---|---|---|
| dayNtlVlm (live 178 stocks) | $10,832 | $461,000 | $1.85B |
| impactPxs bid/ask difference | 0.13bps（BTC） | ― | 101bps（PURR） |

With a single max slippage set value, it will be either too lenient for thin stocks or too strict for thick stocks. Therefore, additions to the allowlist will be determined by the following criteria.

- Estimate the slip in the estimated order size from the thickness of the boards of dayNtlVlm and l2Book.
- Stocks that exceed the threshold of the estimate will not be activated.
- The nominal limit for each stock is derived from the liquidity of that asset. It is not set at a uniform rate.
- After activation, monitor liquidity regularly and stop new orders when the threshold is below.

Daily operation permissions are limited to the suspension of new orders, etc., and do not allow any customer fund transfers. Other risks associated with the right to change SNS codes are subject to 8.3.4.

### 6.4 Privacy

- Do not provide the user ID and corresponding trading account table as a public query.
- Do not return the order content, status, fill results, and cloid without owner authorization. cloid and the final quantity and price may also be reconciliation keys with HL public data.
- Encrypt orders from the client to trading_core (8.3.2).
- Do not log, measure, or send plain text order contents to error messages.
- Do not provide individual orders as an open analysis API.
- Save only the minimum necessary, define a retention period and delete it.
- Design it so that plain text orders cannot be viewed from the management screen.
- Inspect public links through the deposit source, trading account, Agent approval, withdrawal destination, cloid, and history API. Record them by distinguishing between direct transfer links and estimates based on amount and time.
- Specify the entity collecting and retaining IP, device information, and screening information. Do not treat simple IP hashes as anonymous.
- Direct browser connections to HL can convey the source IP, while account-specific subscriptions can communicate the IP and HL account correspondence to HL. Explain this range in D11 and differentiate it from protection and distinction for public observers.

We do not treat achieving confidentiality simply by not creating public queries. We cross-inspect ingress responses, authorized read access, logs, and publicly disclosed fund transfers and HL information. Protection against node operators depends on TEE, and protection against malicious code updates depends on restrictions on change permissions (8.3).

#### 6.4.1 Privacy evaluation of the funding pathway

The evaluation criteria are the reverse of "deposit → confidentialbalance → allocation to user-specific HL accounts → trading → recovery → withdrawal". It will not be accepted only if protection on the deposit side or IP transmission alone.

- **Distinguish observers.** The primary target is a third party that continuously observes public chain, HL public API, and public application information. Additional information held by HL, bridges, credential verification providers, node operators, and SNS permission holders is recorded as a separate threat model. The qualification of the primary target is not extended to guarantee anonymity for all entities.
- **Compare under the same conditions.** Provide the same usage scenarios to both direct deposit method A and adoption method B, and attempt to handle them based on address, public approval, amount, fees, conversion amount, time, and repetitive patterns. Use the internal correct response table only for scoring and do not mix it with the observer's input.
- **Determine the threshold before measuring.** During the observation period, the number of participants, usage frequency, attack procedure, evaluation methods for success rate, false positive rate, and candidate set, and the absolute pass threshold and improvement margin for A are determined using U29. A pass is not granted just because it is slightly improved compared to A, or just because the attack procedure is weak.
- **We also try unfavorable conditions.** Includes low usage, single-use, specific amounts, instant deposit/withdrawal, repeated deposit/withdrawal, partial withdrawal, full withdrawal, and withdrawals involving profit or loss. We do not claim real-world protection only based on the condition of having a large number of hypothetical users.
- **Leave evidence.** Record public observation data, evaluation codes and conditions, response results, and remaining leakage pathways. Do not publish the response tables of real users. The results of testnet/synthetic data are not treated as proof of anonymity in the live environment, and Phase 4 will reconfirm the conditions for the live expected outcome.
- **Decide behavior in low usage situations in advance.** If protection depends on usage, define the reception, waiting, and display policy when those conditions are not met. Do not provide the not met state as a normal protection state. Do not make existing funds permanently inaccessible for withdrawal to maintain privacy, and clearly disclose the possibility of leakage and options at withdrawal.

Fund safety and privacy are evaluated independently. Any of TEE adoption, SNS transfer, the non-disclosure of the response table, or successful withdrawal does not constitute proof of correlation control compliance. If not met, reconfigure the fund flow and reevaluate, and do not proceed with the main deposit. If a change to the D13 target is required, record it as a separate decision.

### 6.5 eligibility and access restrictions

#### 6.5.1 Principles

Canisters cannot obtain the connection source IP in a trustworthy way on their own. Region restrictions for the frontend can be bypassed by calling Canister directly. Therefore, trading-related methods require a short-lived eligibility token with a signature issued by an external compliance gateway.

#### 6.5.2 Token

- The issuer is a gateway managed by the operation.
- Verification items: signature, validity period, linking to user ID, linking to trading account address, judgment result, terms and conditions version.
- Shorten the validity period. Perform re-evaluation regularly.
- Make it impossible to reuse the Principal individually. Make it impossible to switch to another Principal.

#### 6.5.3 Judgment

- Blocking US IP addresses.
- Detection of VPN / proxy / hosting IP.
- Block of restricted countries.
- sanctions screening、wallet screening。
- Regular re-declaration of non-U.S. citizenship.
- Individual wallet suspension by the administrator.
- Limited audit records of decision history (shorten retention period).

#### 6.5.4 Consistency of coercion

The restriction is to use the same implementation across all paths. The same validation function must be passed through in either direct frontend or Canister calls, or in any API that will be added in the future. Direct calls that bypass the UI must exist as test cases.

The necessity of KYC is left to the legal judgment (D2). Implementation should be carried out without KYC while structuring the system to be able to increase the determination strength later. Specifically, consolidate the verification of the eligibility token into a single point and set the required assurance level as a parameter.

"Not US IP" and "Not US citizen" are not synonymous. The level to which you can say it is sufficient without KYC depends on waiting for Phase 0 legal memos.

### 6.6 Operation function

- A safe mode that only allows temporary suspension of the entire system, suspension of new orders, and cancellation/close.
- Trading suspension for stock units, trading suspension for user units.
- Change in risk limit (reduction is immediate, increase up to the user declared value).
- Detection of Hyperliquid API failures and suspension of new orders during failures.
- Monitoring Canister cyclesbalance.
- Monitoring Agentsignature failures.
- Reconciliation job for uncertain orders.
- Record upgrade management and setting changes.

### 6.7 Deposit, fund ledger, withdrawal

- Supported assets and chains are limited to the allowlist. Initially, only HyperCore USDC is supported, and bridges are outside of service (16.2).
- It sets the criteria for deposit confirmation, exclusion of duplicates, treatment of incorrect deposits and unaddressed assets.
- A ledger that maintains double-entry bookkeeping or equivalent storage rules, recording user balances, custody assets, allocation to HL, transfers, withdrawal reservations, and fees. Does not mix user balances.
- Treat deposits, allocation, HL recovery, and withdrawal as separate state machines, storing each with its own unique ID and reconciliation evidence. Do not reuse order cloid as the idempotency basis for fund transfers.
- If balance inconsistency is detected, stop the related new fund transfers and reconcile the affected range. Prohibit unconditional rollbacks of reservations.
- Users will be presented with their own balance and withdrawal status through authentication and encrypted pathways. The scope within which verification can be conducted without disclosing everyone's ledger is determined by U27.
- Even when eligibility expires, a separate decision will be made regarding the suspension of new trading and the return of funds. We do not implement automatic seizure or indefinite implicit lockages, and we finalize the return and hold procedures under legal restrictions in Phase 0.

---

## 7. Non-functional requirements

### 7.1 Security

- Do not generate or store Agentprivate key on a single machine. Use tECDSA.
- Separate the derivation path for each user and do not share the agent key.
- All protected methods are authorized within the Canister. Do not trust the frontend state.
- Reconfirm permissions and order status before and after asynchronous processing.
- Resist to duplicates, timeouts, and uncertain results.
- It retains permissions, order status, and cloid index even after the upgrade.
- Lock and publish the dependency libraries and WASM hashes.
- Undergo third-party audit in Phase 5.

### 7.2 Availability

- New orders will be stopped when the Hyperliquid API is suspended.
- Prioritize cancel / close over new orders.
- If you don't know the external response, do not reorder and reconcile.
- Synchronize the order and position again after recovery.
- Safely process orders during Canister upgrades.
- Stop new orders when TEE anomalies are detected in Confidential Subnet (8.3.3).
- Separate the withdrawal impact caused by funds_vault shutdown, cycles depletion, SNS failure, and HL shutdown. If the completeness of signature and balance is unclear, stop the withdrawal and present the recovery conditions.
- Have a cycle replenishment mechanism that does not use customer funds. Ensure that daily replenishment can continue even if the DAO temporarily fails to function.

### 7.3 Performance

- HFT is not included.
- Within a few seconds from order receipt to Hyperliquid sending.
- You can directly obtain market data display from the browser.
- The trading confirmation status will be verified on the Canister side.
- Batch multiple orders to reduce the number of signature calls (Chapter 9).

### 7.4 Auditing and operation

- Record all changes to settings, permissions, Agent approval/deauthorization, and emergency suspension.
- Do not include plain text order contents in the record.
- Define the retention period and deletion procedure for audit logs.

---

## 8. Architecture

### 8.1 Canister configuration

Separate funds_vault with a funds signature and trading_core with a trading agent. Implement controller constraints with control_guard (16.3).

| Canister | duty | arrangement |
|---|---|---|
| frontend | Public page SSR and asset distribution. Personal data and wallet operations are only for the client. Do not transfer signature and funds API to Workers. | Cloudflare Workers + Static Assets (not Canister) |
| trading_core | Order acceptance, Agent address extraction, Hyperliquidsignature generation, Order state machine, Risk limitation, Hyperliquidreconciliation | Confidential Subnet |
| funds_vault | Customer funds custody, authentication, double-entry ledger, HL allocation/recovery, withdrawal, master signing and reconciliation | Confidential Subnet |
| policy_registry | Allowed countries, prohibited countries, terms and conditions version, compliance token verification key, emergency stop state, ticker allowlist | Confidential Subnet |
| control_guard | Change reservation via SNS with a 7-day delay. The only controller for the eligible Canister. | Subnet is also available normally. Do not store customer information or funds key. |

policy_registry is referred to by trading_core and funds_vault according to their respective purposes. In the case of expired or version differences, new trading and allocation are rejected, and cancellation and withdrawal are determined according to the safety and refund policies of each. It does not include SNS voting in normal processing.

Confirm the ownership of the tECDSAkey Canister before the live deposit. Do not treat the same key as automatically transferring to another Canister. When changes occur, approval from a new Agent or asset transfer may be required. If both Canisters can be updated without restrictions on the same SNS, separation of responsibilities alone does not provide defense against DAO attacks.

### 8.2 Data flow

```
SNS DAO ──control_guard (7-day delay)──▶ funds_vault / trading_core / policy_registry

User connection wallet
  │ deposit                    ▲ withdrawal authorized by the owner
  ▼                         │
funds_vault (confidentialbalance, response table, funds signature)
  │ Funding allocation to HL             ▲ Recovery from HL
  ▼                         │
HL account per user (HL standard fill, margin, settlement)
  ▲
  │ Orders and cancellations signed by trading agents
trading_core ──▶ Hyperliquid API ──▶ Result reconciliation
  ▲
  │ Encrypted order intent / owner authorization
User's browser (Workers only for public SSR and asset distribution)
```

### 8.3 design of confidentiality

#### 8.3.1 Layer of secrecy

Encryption alone does not guarantee confidentiality. The key to decryption is where it occurs. Encrypted orders on the client side are decrypted within the execution environment of trading_core. Therefore, the strength of confidentiality is equivalent to the protection level of the replica running trading_core.

- Confidentiality against public disclosure: Do not disclose the response table and fund ledger; combine reading authorization, encryption responses, log protection, and inspection of public pathways.
- Confidentiality for node operators: Depends on TEE (Confidential Subnet).
- Confidentiality regarding operators and DAOs: Typically, APIs do not grant access permissions for viewing. However, entities that can replace WASM can carry out decryption in a decrypted state. This possibility cannot be eliminated solely by making it a social media platform (8.3.4).

Client-side encryption protects the ingress pathway. Log leakage after decryption is separately prevented. In addition to orders, protection boundaries are also established for withdrawal requests, balance responses, and confidential information between internal Canisters. Pathways that transmit plaintext outside of TEE, as well as decryption keys and storage/recovery handling, are verified under U5/U14.

#### 8.3.2 Order encryption

- The client encrypts the order and fund request and sends it to the Canister address via HPKE. The methods, key authentication, and resend refusal are considered valid as of 16.5, and vetKD is not initially adopted.
- Orders are disclosed to the destination as an enforcement request to HL. Do not include plaintext in any other logs, metrics, panic messages, or error strings. The HTTPS outcall processing path is also considered confidentiality for verification purposes.
- Order parameters after decoding (symbol, direction, quantity, price, order type) are persisted only to the minimum required items for signature. This is because the parameters are needed at the time of signature in a design that separates reception and signature (`Implementation.md` 2.3). This persistence assumes that it is closed to the execution environment and stable memory within TEE.
- The plain text order parameters are saved until settlement. After settlement, they are deleted according to the retention period and only `request_hash`, cloid, state, settled quantity and price, and a summary of Hyperliquid response are left.
- The management screen does not have a path to receive plain text orders.

#### 8.3.3 Confidential Subnet Usage Policy (D7)

Set up on the Confidential Subnet from the early development stage. However, do not enable real trading unless the following prerequisites are met.

Preconditions.

- Verify that all replicas of the configured subnet are running on SEV-SNP.
- Verify the confidentiality of HTTPS outcall, upgrades, state sync, and recovery paths in the real world.
- Publish the WASM hash and controller configuration.
- Stop new orders if you detect an TEE anomaly or attestation inconsistency.
- Disable plain text logs (disable log output in production builds).

There are unresolved open questions. Whether peer attestation between replicas is implemented, whether participating nodes attest first during state sync, whether keys are rotated when a node leaves, whether who can read the state in the recovery path, whether DRAM bus interposition enters the threat model, these are questions raised in the developer forum but have not received answers. In this plan, these are treated as "unresolved premises".

Alternative policy. If the Confidential Subnet cannot be secured during the development period or does not meet the prerequisites, the testnet verification will typically continue on the Subnet and the mainnet deployment will be stopped. The choice not to start live trading on the Subnet is not made. In this case, the mainnet will not be limited until the conditions for live deployment are met with the completion of Phase 4.

#### 8.3.4 Control of canister control rights

Governed by an SNS DAO (D16). In standard SNS management, the SNS root becomes the controller of the app Canister, and WASM or controller configuration can be changed with approved proposals. Do not leave additional controllers or hidden management signature pathways for individual operators.

Direct control of SNS root prevents developers from making unilateral changes, but malicious changes by DAO cannot be prevented. Since confidential canister processes plain text, the change permissions can also lead to data disclosure permissions. Confirm and verify the following before the production phase.

- Evaluate the allocation of voting rights, centralization of delegation, and approval conditions, and include governance attacks on deposited assets as a threat model. SNS launch and token allocation and sale conditions will be determined separately (U28).
- We are publishing the WASM hash, reproducible builds, and controller configuration and change proposals. We are not posting customer plaintext balance and destination addresses as public proposals.
- Set a 7-day forced suspension with control_guard in 16.3. The voting period for SNS is not an execution timelock, but also includes early decision.
- We are verifying whether you can bypass the delay from direct upgrade, controller change, reinstallation, stop/delete, or changing the restriction settings themselves. It is not enough to just write a waiting time within the same Canister that can be changed.
- Adopt a non-modifiable guard configuration. This is not a guarantee automatically provided by SNS standard features, but it requires feasibility verification and auditing to restrict the bug fixes and recovery of the guard.
- Do not depend on user withdrawals on DAO voting. Before dangerous changes, we will verify the time and path for trading suspension, cancellation, settlement, and fund recovery. We do not promise that you can withdraw immediately during HL suspension or margin restrictions.
- Emergency operational authority will be limited to suspension and strengthening restrictions, and will not allow voluntary transfers, destination changes, disclosure of confidential information, or skipping updates. Recovery during fund layer suspension will follow U26.
- Do not count the transfer restrictions on SNS Treasury as restrictions on assets in external HL accounts or funds_vault.

Practice changes, exits, and recovery in Phase 2, and audit the detour paths in Phase 5. Do not express that "even the DAO cannot steal funds / cannot read internal information" until the constraints can be implemented and verified.

### 8.4 tECDSA

#### 8.4.1 key derivation

- Separate the derivation path for random opaque account_id, purpose, and Agent generation. Do not include the public user_id directly in the path. Store the corresponding data as confidential data. Do not consider the extraction of the public key as a secret‑keeping mechanism.
- The agent address is calculated from the derived public key and presented to the user.
- The entire private key does not exist in either a single node or a canister. A canister can only require a signature.
- Store the output results for each agent generation. After expiration, rotate to the new generation and do not re-authorize the old address. Separate and measure the costs of acquisition and signature, and separate the path and ownership canister of the funds key and agent key.

#### 8.4.2 signature

- The signature method is threshold ECDSA of secp256k1.
- We need to recover v (recovery id) from the signature. Since v is not included in the response of threshold ECDSA, we try the candidates and choose the one that matches the public key. This process is considered a verification item in Phase 1.
- The retry signature for unsubmitted action is limited to the same signing payload and invalidates the old callback at the worker epoch. The signature call exactly-once is not guaranteed by the request ID alone.

#### 8.4.3 Agent's expiration date

Hyperliquid agents expire. Third-party SDK guidance describes a default of 90 days and extension up to 180 days by adding `valid_until <timestamp>` to the agent name. Confirm the exact specification and constraints with official documentation and testnet measurements in Phase 1.

Operational consequences.

- Display the expiration date and re-approval guide on the UI at all times.
- Notify the user before the expiration date.
- If you detect expired, stop the new order.
- Re-approval is performed by funds_vault. It verifies the method of not reusing expired Agent addresses and switching to new keygeneration.

### 8.5 Hyperliquid integration

#### 8.5.1 signature

Hyperliquid's signature is strictly determined by field order, msgpack encoding, numeric representation (handling integers and decimals), action hash, wrapping at agentsignature, EIP-712 domain and type, nonce, and presence of vaultAddress. If it is even slightly different in any place, the signature will not be valid.

- The official SDK and the consistency of serialization, hash, and recovery address for the same input, signature verification and acceptance in HL are the qualifying conditions. Since the signature randomness is different between tECDSA and SDK, the consistency of the signature byte sequence itself is not required (Phase 1 Go/No-Go).
- Fix the test vector in the repository and run it as a regression test always.
- Monitor API specification changes and stop new orders if you detect inconsistencies.
- The implementation is done in Rust and separated from the frontend.

#### 8.5.2 HTTPS outcall

- POSTs that involve state changes are not sent per replica. Use non-replicated outcall for single requests.
- Do not use the response as the basis for success. The response will only be treated as information that "may have been accepted".
- Acceptance is idempotent with user-level request ID and text fingerprint, and external transmission is managed by immutable action/nonce and pre-transmission record. cloid does not guarantee permanent duplicate elimination by itself.
- Before sending, re-verify permissions, policy, expiration date, and risks, then permanently establish dispatching before issuing a POST. For unknown external effects, reconcile instead of resend.
- Don't use transform, just extract the required field. Be careful with the response size limit.

#### 8.5.3 nonce

Hyperliquid requires a unique nonce for each signer. Since the Canister generates the nonce, it satisfies the following constraints.

- For each signer, atomically ensure max(now_ms, last_nonce + 1) and verify the valid window. The nonce is not in the child order but in the signature action unit.
- It will be sustainable and continue after the upgrade.
- Do not automatically reconstruct the action of unknown outcome with a new nonce. First, reconcile the external effects.
- The nonce range restriction (valid window) is measured in Phase 1.

#### 8.5.4 asset index and ticker metadata

The asset index that refers to the ticker in Hyperliquid's order action is an integer, and in perpetual futures it is a subscript in the `universe` array of the `meta` response. This value is included in the msgpack of the signing payload.

Measurements on 2026-09-18 found 234 `universe` entries, including 56 with `isDelisted: true`, scattered across indices 3 through 202. The array is append-only; delisted assets retain their positions.

Therefore, keep the following invariant conditions.

- Do not compress `universe`. Removing deprecated elements will cause all subsequent annotations to be shifted.
- The asset index is resolved from the most recent `meta`. It does not rely on the permanent response table as the sole basis.
- If the `meta` retrieval fails or the retrieval result does not match the previous one, reject the new order. Do not complete by guessing.
- Save the resolved asset index to the order record and verify the match during reconciliation.
- Reject orders for unknown tickers, discontinued stocks, and stocks outside the allowlist.

If this invariant condition is violated, the order will not be an error and can be correctly signed and filled for another symbol. The more symbols increase, the wider the range of influence this single defect can have.

### 8.6 Order status machine

The action transmission has the following statuses: queued / signing / signed / dispatching / reconciled / unknown / aborted. The order lifecycle has the following statuses separately: pending / open / partially_filled / filled / cancelled / rejected / unknown. The reconciliation of the action is not the completion of the fill.

- Permanently maintain dispatching before POST, and thereafter only reconciliation. Do not return from callback trap, timeout, or upgrade to unsubmitted status.
- After each await, check the worker epoch and status in the CAS. Re-verify cancellation, kill-switch, and deadline changes in the signature before sending them.
- Separate the cancellation of unsubmitted cancellation and the cancel action to HL. expiresAfter and local deadline do not mean the cancellation of existing orders.
- Instead of just marking unknown as expired on the terminal, keep the reservation. Just not found does not prove that it has not been executed.
- Batch orders are limited to compatible child orders on the same account or agent generation, and the results are reconciled by child order.
- Service internal risk reservations are counted as parallel orders, but they are distinguished from the strict upper limit of the entire account, including HL trading and other agents, directly.

### 8.7 Sustainability and upgrades

- Place orders, cloid indexing, nonce, Agent metadata, eligibility verification results, risk limits, and kill-switch state in stable memory.
- Order parameters are permanent and irreversible to 8.3.2. funds_vault is permanent to a different canonical as funds_ledger, deposit identifier, owner authorization, allocation, withdrawal reservation, transfer result, keygeneration. Order history deletion rules are not applied to funds_ledger.
- Verify compatibility of the condition before and after the upgrade. Perform a real-time restoration test in Phase 2.
- Recovery test targets: Upgrade interruption, state rollback, pending order re-reconciliation, cloid indexing consistency.
- External transfers cannot be canceled even after restoring the local state. To prevent double spending due to restoring the old ledger, after restoration, reconcile the real asset and transfer history and explicitly determine the resumption of fund transfers.

### 8.8 Verification requirements (Go/No-Go Phase 1)

You will not proceed to the next phase unless all of the following are fulfilled.

1. The Rust implementation matches the official SDK in serialization, hash, and recovery address, and tECDSAsignature can be verified and accepted.
2. You can recover v from tECDSAsignature.
3. Orders and cancellations are completed on the testnet.
4. The agent registration and expiration flow is established.
5. The idempotency of request ID, automatic resend of sent action is prohibited, and non-reuse of Agent generation is established.
6. Inject POST response abandonment, callback trap, delayed callback, and signature cancellation to recover by reconciliation or safe unknown maintenance.
7. HTTPS outcall, upgrade, and state recovery work on Confidential Subnet.
8. By tracking testnet deposits, HL allocation, recovery, and withdrawal through the selected funding channel, you can retain your balance without resending it when external effects are unknown.
9. The boundary between Funds signature and Agentsignature, the change permissions of SNS, and the feasibility of exit/recovery design can be confirmed. The SNS control test is conducted in the local/test environment, and the actual launch is not based on the premise of a spike.
10. Regarding the reverse funding flow in 6.4.1, perform A/B comparisons using the observer and threshold defined in Phase 0 and meet the initial qualifying conditions for privacy improvement. If easy response from amounts and times is still required, redesign the funding pathway.

This qualification is a continuation judgment for Spike and does not grant permission for actual fund acceptance. In Phase 3 to 4, we will reevaluate the realistic usage conditions and final implementation, ensuring that the conditions for fund safety, legal compliance, and auditing are also met.

---

## 9. Cost model

### 9.1 Unit price

The following are reference values for the old order path, and will be re-measured after reflecting fee adjustments, outcall mode, and subnet configuration. In B, deposit confirmation, fund signature, HL allocation recovery, withdrawal reconciliation, confidential ledger, and SNS operation costs will be added. Cost reduction due to aggregation into a single HL account is not included in the estimate.

- Cycles are pegged to XDR. 1 trillion cycles = 1 XDR. As of 2025, the rate is 1 XDR ≈ $1.35–1.37. It fluctuates with the exchange rate.
- The real `sign_with_ecdsa` is about 26.15B cycles per signature (about $0.035).
- The HTTPS outcall for a 13-node subnet is the fixed cost of `(3,000,000 + 60,000 × n) × n` cycles plus a per-request/per-response byte charge. For n=13, it is 49.14M cycles (approximately $0.00007). For n=7, it is 23.94M cycles.
- As a reference, Chain Fusion Signer charges 37B cycles per standard signature.

### 9.2 Approximate amount per order

| an item | cycles | a note |
|---|---|---|
| signature (order) | About 26.15B | Maximum cost item |
| HTTPS outcall（POST） | About 0.05B | Depends on response size |
| reconciliation (/info POST) | About 0.05B | More than once |
| Execution, ingress, storage | Several M~several dozen M | Can be ignored |
| Total | About 26.3B | About $0.036 |

### 9.3 Conclusion

- If you sign up orders and cancellations separately, the calculation doesn't match in high-frequency trading.
- Batch multiple orders into one action. Hyperliquid accepts multiple orders in one request, so we batch as much as possible.
- Cancel All is sent in the same account and Agent's limited batch. Depending on the number limit, there may be multiple signatures.
- Signature costs become direct marginal costs relative to the revenue model (D5 builder fee). Measure them in Phase 3 and determine the cost line.
- Cyclesbalance depletion leads to service outage. Implement balance monitoring and automatic top-up. However, it is completely separated from user assets.

---

## 10. Risks and countermeasures

### 10.1 Technology

| risk | Influence | Countermeasure |
|---|---|---|
| Inconsistency with Hyperliquidsignature specifications | The order is not being processed at all. | Require test vector matching with the official SDK as a mandatory condition. |
| API specification change | Frequent order failures | Specification monitoring, stopping new orders when inconsistencies are detected, tracking SDK updates |
| signature cost | Declining profitability | Batch processing, optimization of cancel-only operation, real-time measurement in Phase 3 |
| Delay in tECDSAsignature | Opportunity loss | Excludes HFT. Design to tolerate delays in the UI. |
| nonce collision | Order refusal | Guarantee of uniformity and sustainability, reconstruction in case of conflict |
| Asset index resolution error | An unintended order for an unwanted stock fills | Do not compress `universe`, resolve it each time from meta, reject if inconsistent (8.5.4) |
| Slippage on a thin stock | Fill at an unexpected price | Unlocking based on liquidity standards, nominal limits per stock, and estimates via l2Book (6.3.1) |
| Positions under suspension or settlement | Payment cannot be processed / Forced settlement | Monitoring of the deprecation flag, suspension of new orders when deprecation detection is detected, notification of position consolidation (Chapter 11 Phase 3) |
| Uncertain response | Double order/state inconsistency | cloidreconciliation, resend prohibited, explicit indication of unknown status |
| Confidential Subnet unresolved issues | Confidentiality cannot be claimed | Make the prerequisites of 8.3.3 the conditions for actual deployment. If not met, stop. |
| Availability of Confidential Subnet | Service suspension | Early real-world verification, cancel-only mode, explicit alternative policy |
| Violation of control rights of canisters and SNS decision rights | Unauthorized orders, outflow of entrusted funds, disclosure of internal information | Limited change rights in 8.3.4, exit opportunities, public verification and audit |
| Cycle depletion | Trading suspension and customer funds withdrawal suspension | Replenishment by operating budget, balance monitoring, recovery exercises |
| double counting, duplicate withdrawal, recovery of old ledger | Lack of funds, loss of impact on other users | Financial status machine, transfer reservation, external effect reconciliation, resumption determination |
| Disruption of HL accounts and fund transfer channels | Recovery cannot / withdrawal delay | Asset allocation and traceability per user, recovery procedures per dependent asset |
| Funds ledger leakage | User withdrawal and HL account linkage | confidential execution, reading authorization, encryption communication, restriction of modification rights |

### 10.2 Business

- Correlation can also be inferred from the amount and time of public withdrawal in B. → We will verify improvements with U23/U29 on 6.4.1. If easy adjustments remain, we will not accept them without explanation; we will redesign and reevaluate them and stop the actual transition.
- geofence alone cannot prove the exclusion of US persons. → Based on the legal memo, the necessary additional measures will be finalized in Phase 0.
- According to Hyperliquid's terms of service, it is necessary to confirm whether third-party services using the agent are permitted. → Phase 0 confirmation item.
- Since revenue depends on the builder fee, changes to Hyperliquid's fee specifications will directly be a revenue risk.

### 10.3 Legal affairs

- Based on the premise of Canister custody and SNS governance, we evaluate the legal aspects of the classification and operation of custody, broker, IB, etc., along with the roles of the DAO. We do not reuse the evaluation of the former Agent-only proposal.
- We will document the change permissions for common custody locations, bridges, SNS, HL master keys, and the ability for users to withdraw independently. We will not assume either "SNS = not subject to regulation" or "decentralized key = non-custody." We will reevaluate the feasibility of implementing the D2 policy of no KYC and will proceed with the official launch after written confirmation.
- Determine the target product and target country and obtain a written answer.
- Fix the prohibited operation as a specification (3.2).
- Explicitly state the trust assumptions of Confidential Subnet (8.3).
- Explicitly highlight the risk that not being US IP and not being an American citizen are not synonymous.

### 10.4 Unacceptable risks

- Accepting customer funds while the U15/U23/U24/U26 live verification gate is not met. Granting trading_core with a funding key or any withdrawal signature permissions.
- Normal live trading on Subnet.
- Output of log for plain text orders.
- Canister control by a single live key.
- By basing it on SNSization, conceal the risk of fund outflows, information disclosure, and withdrawal suspension due to change permissions.
- Only verify the safety of the funds custody and accept the actual deposit with D13's privacy improvement not met.
- Double execution of the misappropriation of customer funds to the DAO operation budget and an unconfirmed external transfer.
- Deciding to start the actual operation without KYC and without legal memos.

---

## 11. a road map

Add funds ledger, withdrawal, recovery, and SNS governance with D15/D16. Update the following processes for the B version. Do not use the period of the old A as a current commitment; re-estimate after design and verification of Phase 0–1.

### Phase 0 - Funding and governance design, legal and threat models

- The B funding pathways, HL master permissions, signatureCanister, and withdrawals/reinstalations are finalized in U23/U24/U26. Develop an observer model that distinguishes between direct links and correlation inferences.
- Before measuring the behavior in low usage, determine the observation conditions for A/B comparison, correlation methods such as amount and time, the pass threshold, and U29 for A/B comparison.
- Test the allowance, withdrawal, and bypass prevention with the 16.3 guard configuration. The SNS voting rights allocation and launch conditions will remain as the main gate for U28.
- Design a funding state machine, backing reconciliation, audit scope, and align Implementation.md. Do not assume permissions based on NEAR's unconfirmed perp implementation.

- Determine the target product and the target country.
- Determine the location of the operating entity (D8).
- Check the scope of involvement of the U.S. corporation.
- Check the impact of Canister's funding custody and SNS governance on the analysis of custody / broker / IB, and confirm the feasibility of D2 execution.
- Check whether third-party services using the agent are permitted under Hyperliquid's terms of use.
- Fix the prohibited operation as a specification.
- Explicitly state the trust assumptions of Confidential Subnet.
- Get a written memo from a lawyer who is familiar with U.S. derivative regulation.

Output: Legal memo, key/controller permission diagram, fund data flow, ledger/withdrawal status machine, change/exit/recovery design, privacy evaluation specifications (observer/comparison conditions/threshold), list of prohibited functions.

### Phase 1 — Technical Spike

The first verification unit encompasses not only the success of the order but also the information disclosed when the funds are transferred back and the overall funding rights.

- On the testnet, deposit, confidential balance recording, allocation to user-specific HL accounts, trading, recovery, and withdrawal are completed in a loop, and all pathways are recorded.
- We will verify the possibility of fund transfers and response tables that can be possible with SNS updates, as well as the signature entity at each stage, the recovery entity at the time of suspension.
- Measure the response based on the amount and time and privacy improvement by directly comparing with deposit method A in the evaluation specifications of 6.4.1.
- Reimplement Hyperliquidsignature in Rust.
- Verify serialization, hash, and recovery address matching with the official SDK, and verify and accept tECDSAsignature.
- Restore v from tECDSAsignature.
- Place orders, cancel orders, and send batches on Hyperliquid testnet.
- Measure the flow of agent registration, validity period, and expiration.
- Verify the idempotency of reception by request ID and the transmission management and reconciliation by action/nonce/cloid.
- Verify HTTPS outcall, upgrade, and state recovery on a Confidential Subnet.
- Verify HPKE's key authentication, encryption, revocation, and response protection adopted in 16.5.
- Demonstrate the deposit of funds, HL allocation, recovery, withdrawal on testnet and reconciliation of uncertain results.
- Verify SNS control, change restrictions, and exit period bypass in the local/test environment.
- Report separately on the improvement of fund safety and privacy. Distinguish between "passed", "not met", and "unconfirmed", and do not treat "unconfirmed" as a success.

Go/No-Go: All items of 8.8.

### Phase 2 — Single-user testnet MVP

- 1 user = 1 trading dedicated account.
- Market / Limit / Cancel / Cancel All / Close / SL / TP（reduce-only）。
- Order and position reconciliation.
- Dead-man's switch, trading suspension, Agent failure, emergency full cancellation.
- Recovery test after upgrade.
- Implement funds_vault's owner authorization, ledger reservation, withdrawal, and HL funds recovery.
- Verify that withdrawal and optional funds signature cannot be done from trading_core.
- Test for duplicate deposit, duplicate withdrawal, response loss, recovery from old ledger, and malicious changes.

### Phase 3 — Multi-user closed beta（testnet）

- Extraction of agent key by user.
- Encrypted orders.
- Rate restrictions, position limits.
- Management of the stock allowlist. Unrestricted determination based on liquidity standards (6.3.1), nominal limits per stock, periodic observation and automatic suspension.
- `universe` Conformity test for the asset index that scans all entries (8.5.4). Includes discontinued tickers, unknown tags, and refusals outside the allowlist.
- The guide for detecting the withdrawal of listed companies and organizing the positions of withdrawn stocks.
- nonce competition countermeasures.
- partial failure, timeout, duplicate intrusion test of failures.
- Audit log.
- Verification of eligibility token (6.5).
- Implementation of the builder fee and cost estimation.
- Disruption injection on testnet.
- Verify the separation and supporting reconciliation of balance, fund allocation, and withdrawal reservation between users.
- We will verify the concentration, delegation, early resolution of SNS voting rights, change delay, customer withdrawal, and operation fund separation.
- Measure the expenses including fund ledger, signature, reconciliation, SNS operation.
- In addition to conditions such as low usage, repetitive withdrawals, and exit with losses and gains, re-execute the A/B evaluation in 6.4.1 for multiple users.

### Phase 4 - Meeting the conditions for the actual deployment

With D7, development will be done on Confidential Subnet from the beginning. This phase is not a migration, but a work that meets the prerequisites of 8.3.3.

- Check the SEV-SNP operation of all replicas.
- Verify the confidentiality of HTTPS outcall, upgrade, state sync, and recovery paths in the real world.
- Confirm the complete elimination of plain text logs.
- Publish the WASM hash and controller configuration.
- Implement and test the automatic stop of new orders when TEE is abnormal.
- Implement control of control rights (8.3.4).
- Prepare the specific steps for SNS launch and permission transfer. Execution requires separate approval. Verify the bypass route with the actual controller before the live deposit.
- Complete a fund recovery exercise that assumes the shutdown of UI, Canister, SNS, and HL.
- By checking the basis for the establishment of U29's privacy conditions with the final configuration and usage and observation information expected for the actual use. Do not use the trading volume of the testnet as the basis for the actual anonymity, and do not start the actual deposit when not met.

### Phase 5 - Audit and limited mainnet

- Canister security audit.
- Hyperliquidsignature difference audit.
- Audit of permissions and upgrades.
- Audit of the fund ledger, withdrawal status machine, SNS change permissions, exit/recovery and information disclosure pathways.
- Invitation-only release with deposit assets, user allocation, and trading limits. Funds will only be accepted after both the security and privacy improvement gates are verified, followed by audit, legal, and authorization transfer confirmation.
- Test in cancel-only mode during major disabilities.
- Bug bounty.
- Legal re-review.

### Estimated period

The following is the reference estimate for A that was not adopted, and it is not the delivery deadline for B. Including B's fund layer, withdrawal, recovery, and SNS design and audit, a re-estimate will be made after Phase 0 to 1.

- Until testnet MVP (Phase 1~2): about 8~11 weeks.
- Until the limited mainnet (Phase 3~5): about 5~7 months.

Due to the use of D7's Confidential Subnet, it has been extended from the initial plan (4 to 6 months).

The extension of D3's full perps support is mainly due to Phase 3. The reasons for the extension are not the implementation of each stock, but the three points of defense against immutable shared conditions, the extraction of liquidity parameters, and the operation of stock listing cessation. When shortening, the judgment to limit the allowlist to major stocks is the most effective. The reuse of signature and state machines is not dependent on the number of stocks, so this limitation does not require architectural changes.

---

## 12. MVP completion conditions (acceptance criteria for the transition to closed beta)

- The design for U15/U23/U24/U26 is complete, and Implementation.md for B and the permission table and acceptance criteria are also available.
- Regarding user ID, connection wallet, HL account, and the reverse funding pathways, demonstrate improvements to the deposit method directly using the observer/threshold approach from 6.4.1. Just creating a dedicated address or making the response table non-public does not qualify.
- If easy response based on amount and time etc. remains, we will classify it as D13not met, and even if the fund security is qualified, we will not proceed to the actual trial. Record the evidence of evaluation, behavior during low usage, and residual risk.
- Customer funds and DAO operation funds are separated, and there is no transfer route to the operation budget.
- funds_vault can verify owner authorization, balance, binding, destination, and nonce and complete a normal withdrawal without DAO voting.
- Deposit double counting, duplicate withdrawal, and unauthorized release of reservations do not occur. HL allocation and transfers can be reconciled with the ledger and backing using the same criteria, including assets in transit.
- Test for response loss, Canister stop, cycles depletion, upgrade, and recovery from old state, and do not run external transfer in duplicate.
- With the trading_core Agent, withdrawals cannot be performed, and arbitrary signatures in the funds_vault cannot be requested.
- Trading stops and Agent invalidation will be activated, allowing you to complete the processing of existing orders and positions separately.
- Verify SNS change permissions, bypass routes, and withdrawal restrictions in the test environment. Before receiving actual funds, also confirm the actual controller configuration, the remaining permissions of the operators, and the U28 conditions.
- If the official Hyperliquid SDK and signing payloadhash and recovery address match, the signature will be verified and accepted.
- No duplicate orders occur (resend test of the same cloid).
- You can restore the order status even if you lose the POST response.
- You can restore the status after the Canister upgrade.
- Cannot execute the order method from the US IP.
- Access restrictions also work with direct calls bypassed through the UI.
- The order details are not leaked to logs, metrics, or public queries.
- Only cancel / close can be maintained in emergencies.
- The kill-switch works on a per-brand basis.
- There are no serious issues in the third-party security audit.
- I have received a written response from the law firm regarding the target country and target products.

---

## 13. Things that cannot be made with MVP

- omnibus account that aggregates all users' HL positions, and a unique position and clearing ledger.
- Unlimited support for deposits and withdrawals to multiple assets and multiple chains. MVP will limit the selection path to U23 (deposits, confidential balance ledger, and withdrawals are included in MVP).
- User-to-user transfer.
- Unique order book, unique perpetual contract.
- Copy trading, social trading.
- Complete anonymization.
- Unique token initiatives that exceed the scope necessary for SNS governance. The design of SNS and its governance tokens are included in the scope of the subject, but the execution of issuance, sale, and launch is separately approved.
- Mobile app.
- Low-latency execution and collocation for HFT.
- TWAP, Rebalancing, Complex Strategy Marketplace (candidates after Phase 5).
- The official adoption of the unverified HL master key generation and recovery method in U24. The generation itself by Canister is not prohibited in any way.

---

## 14. Decision status and remaining gates

Maintain the following U numbers for tracking purposes. Distinguish between the selected policy and the remaining items to be verified externally. Do not stop the start of local implementation until the full issue is resolved.

- **Implementation available:** synthetic data, local/PocketIC, HL testnet. Real funds and confidential information of real customers are not used.
- **Waiting for real-time verification:** U6/U7/U8/U14, confirmation methods for transfer events, SNS coordination of guard, correlation resistance. Do not confuse design values with the guarantee of external services.
- **Required before the actual event:** Business, legal, and external provider for U1/U2/U3/U4, actual event fees and storage obligations for U11/U12, SNS allocation and launch for U28. We will not recruit or accept real customers until unresolved.

| # | Decision/Remaining items | Next verification point | charge |
|---|---|---|---|
| U1 | The operating entity, location, and involvement in the United States are undetermined. Development of synthetic data is possible. | Before actual customer recruitment and reception | business |
| U2 | Development is a synthetic attribute without KYC. The necessity and level of actual KYC are determined by legal judgment. | Before actual customer recruitment and reception | Law |
| U3 | The funds custody, account allocation, and Agent usage availability under HL regulations are not confirmed. | Before actual customer recruitment and reception | Law |
| U4 | For development test issuers, contract and register external issuers for the actual production. | Before the performance | business |
| U5 | HPKE recruitment, vetKD not recruited (16.5) | Key authentication and decryption exam in Phase 1 | Implementation |
| U6 | Can we ensure a subnet for the development of Confidential Subnet, and what is the 7-node charging standard? | Phase 1 | Implementation |
| U7 | Valid for 30 days, new generation switching on the 27th day. Verify HL compatibility | Phase 1 | Implementation |
| U8 | The behavior of Hyperliquid when the nonce's valid window overlaps | Phase 1 | Implementation |
| U9 | positionTpsl, reduce-only. Complex brackets are not eligible for initial use. | Operation test in Phase 2 | Implementation |
| U10 | Starting from BTC/ETH. Additional unlocking will be after the liquidity evaluation on 6.3.1. | By additional stocks | a product |
| U11 | testnet fee=0. The live rate and consent display are pending approval. | Before the performance | business |
| U12 | The default deletion of development is 16.5. The obligation to save in the original form is determined by legal judgment. | Before the performance | Legal affairs and implementation |
| U13 | Default OFF. Explicitly indicates the risk of protection order cancellation. | Stop exam in Phase 2 | Implementation |
| U14 | Verification methods such as attestation for Confidential Subnet, and the scope of guarantee for state sync, recovery, and outcall. Do not assume that undetectable anomalies can be detected. | Verification plan is completed in Phase 0~1 before the actual deployment | Implementation and business |
| U15 | Adopting immutable guard + 7-day grace period (16.3). SNS connection and bypass prevention are unverified. | Phase 1 verification and pre-production audit | Implementation |
| U23 | HyperCore USDC, common custody → transfer between independent accounts (16.2). Allocation correlation countermeasures will be finalized after the comparative test. | Phase 1 | Implementation |
| U24 | EOA authentication, vault master, withdrawalsignature every time. No guarantee of withdrawal from the same EOA (16.1) | Phase 1-2 examination and legal affairs before the actual test | Implementation and legal affairs |
| U25 | Personal data is via Canister, only market status is directly connected to HL (16.5) | Phase 1 communication inspection | Implementation |
| U26 | The stop range, cycles, and alternative UI are decided at 16.4. Fund recovery requires a disruption exercise. | Phase 2 | Implementation and operation |
| U27 | double-entry ledger, equity separation, in transit reservation, unknown holding (16.2). The API compatibility of confirmed evidence is unverified. | Phase 1〜3 | Implementation and audit |
| U28 | Adopt guard. SNS allocation, sales, launch, and final transfer are not approved. | Before receiving the actual funds | Business, implementation, legal affairs |
| U29 | Observer, A/B0/B1, set the number of people, period, and success rate criteria at 16.6. The achievement result is not measured. | Phase 1 initial judgment and pre-show re-verification | Implementation and audit |

Manage U16–U22 in `Implementation.md` chapter 12.

---

### Additional acceptance conditions due to design revisions

- Link the ownership of the caller and connection wallet with a one-time signature challenge. The response to HL master is verified using U24's management method, and Agent approval and builder fee approval are reconciled separately.
- Perform a recovery exercise that includes nonce, intent to send, and signatureoutbox. Do not resume sending while rolling back, assuming that the entire DB can be restored from HL.
- Continue reconciliation of open/unknown even if the browser is closed. Stop orders with increased risk if the status is old.
- If the reliability boundary of non-replicated reading and countermeasures against tampering and stale are undecided, we will not proceed to the actual test.
- For detailed order schema, fencing, backup, and cancellation test specifications, please refer to Chapters 4, 5, and 9 of Implementation. When there is a conflict with funds or account permissions, prioritize this Plan and reconcile it before implementation.

## 15. reference materials

ICP official materials (SNS change permissions and the boundary of confidentiality, confirmed on 2026-09-18):

- [SNS management: Upgrade, controller transfer, early decision, Treasurytransfer](https://docs.internetcomputer.org/guides/governance/managing/)
- [ICP Security Model: controller risk, memory confidentiality, code responsibility](https://docs.internetcomputer.org/concepts/security/)
- [SNS Launch: Transfer to SNS root of the App Canister](https://docs.internetcomputer.org/guides/governance/launching/)

NEAR official documentation (verified on 2026-09-18. Distinguish between general Intents specifications and speculation about the near.com perps implementation):

- [Confidential Intents: FAR / Treasury / IMT / Private PoA Bridge](https://docs.near-intents.org/integration/market-makers/confidential-intents)
- [Confidential Swaps: near.com integration example to keep confidential balance](https://docs.near-intents.org/integration/distribution-channels/1click-api/quickstart/confidential-swaps)
- [Hyperliquid: Integration of deposit and withdrawal to perpsbalance of specified HL accounts](https://docs.near-intents.org/integration/distribution-channels/1click-api/hyperliquid)
- [near.com Terms: The scope of guarantee of Confidential Mode](https://near.com/terms)

- [Hyperliquid: Nonces and API wallets](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets)
- [Hyperliquid: Exchange endpoint (usdSend, Agent approval, SL/TP, scheduleCancel)](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint)
- [Hyperliquid: Bridge2 (CCTP priority, old bridge not recommended)](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/bridge2)
- [RFC 9180: HPKE Cipher Suite](https://www.rfc-editor.org/rfc/rfc9180.html#section-7)
- [hyperliquid SDK: Agent wallets and vaults](https://github.com/nktkas/hyperliquid/blob/main/docs/guides/agent-wallets-and-vaults.md)
- [ICP: Cycles (XDR peg, replication factor)](https://docs.internetcomputer.org/concepts/cycles/)
- [ICP: Cycle costs (signature/HTTPS outcall price)](https://docs.internetcomputer.org/references/cycle-costs/)
- [ICP: Cycles cost formulas (HTTPS outcall calculation formula)](https://oa7fk-maaaa-aaaam-abgka-cai.raw.icp0.io/docs/references/cycles-cost-formulas)
- [DFINITY Forum: Questions on confidential (SEV-SNP) subnets](https://forum.dfinity.org/t/questions-on-confidential-sev-snp-subnets/75232)
- [DFINITY Forum: Are chain-key signing costs sustainable for wallets and high-frequency applications?](https://forum.dfinity.org/t/are-chain-key-signing-costs-sustainable-for-wallets-and-high-frequency-applications-the-oisy-example/75316)

---

## 16. Confirmed specifications at the start of installation

This chapter is an implementation decision as of 2026-09-18. Referencing the U number in the text should be interpreted as a reference to this specification and the remaining verification conditions. When changing values, record the reason and the test results.

### 16.1 authentication, key, recovery (U24)

- Initial support is only for EOA with secp256k1 like MetaMask. Generate a short-lived ICsignatureIdentity in the browser and bind the connected EOA and its Principal with EIP-712 challenge. The challenge includes origin, network, canister ID, purpose, nonce, and a 5-minute expiration period, and is accepted only once. The session expires after 30 minutes and can be invalidated by logging out. The secret session key is only stored in memory.
- User ID and trading account ID are cryptographic random numbers. EOA/Principal is not embedded in the public derivation path or cloid. Re-login links the same EOA to the same user ID.
- funds_vault manages the master keys for shared reserve accounts and per-user trading accounts using tECDSA. Each user's account is an independent master account and is not a shared master's HL sub-account. trading_core only manages agent keys for each account and for each generation.
- For withdrawals, EOAsignature is required each time to restrict the amount, asset, recipient, network, nonce, and expiration date. The initial withdrawal destination is only the HL account of the authentication EOA. Transfers to third parties, changes to registered EOAs, and authentication resets by the operator are not implemented. The fund transfer nonce and login challenge nonce are managed separately.
- We do not provide compensation via email or other means in case of loss of EOA. We do not guarantee user-only withdrawal when the canister is suspended. We do not confuse re-login with fund recovery. We display this restriction before depositing.
- Agent is valid for 30 days, and from the 27th, it will try to switch to a new generation. Approval and revocation are handled by funds_vault, and it will be activated after HLreconciliation. Until the API compatibility with time-specified deadlines is verified on testnet, this value will not be treated as HL's guarantee.

### 16.2 Initial capital pathways and ledger (U23/U27)

- The initial assets are only USDC on HyperCore. Starting from testnet, the initial UI will be limited to deposits from users' existing HL accounts. It will not be an UI that can automatically transfer USDC from other chains just by connecting to MetaMask.
- The basic path is "HL account of the person → shared reserve account not used for trading → HL trading account per user → shared reserve account → HL account of the person". The standard `usdSend` is verified as the target. Deposits from the person are signed by the person, and thereafter the master signing of funds_vault. The activation of a new account, minimum amount, transfer fees, and acquisition of confirmation events are measured in Phase 1.
- Withdrawals from other chains, proprietary bridges, ckUSDC, and proprietary swap tokens will not be implemented initially. Transfers between external wallets and the user's HL account will be explicitly stated to be outside of this service. The traditional Arbitrum Bridge2 will not be replaced by a newly designed version.
- Do not allow trading agents or positions to be held in a shared reserve account. In user-specific accounts, HL standard margin and liquidation are used. Do not offset other users' margin and losses, and do not transfer losses to others' balances.
- Separate unallocated balance, withdrawal reservation, in transit asset, and user-specific HL equity. Do not double-count common custody asset and user-specific account asset. Do not credit the unsettled PNL as a withdrawable balance. The recovery amount is determined by the actual withdrawable amount of HL and the transfer result.
- The fund ledger is a double-entry journal using integers as the smallest unit of USDC, incorporating external event IDs uniquely. It permanently stores fund requests, reservations, signatureoutbox, and reconciliation receipts, and does not include external calls within DB transactions. Unknowns are not released or resend only based on time elapsed.
- In the event of ledger discrepancies or insufficient liquidity, new allocation and risk increases will be halted. Withdrawals are not uniformly halted until they can be securely backed by and verified by owner authorization, but no payments for unknown funds will be made. We do not claim that the completeness of the debt can be proven solely through public balance proofs.

### 16.3 Change rights of SNS (U15/U28)

- The target for the live version is `SNS governance → control_guard → funds_vault / trading_core / policy_registry`. As the only controller for the Canister targeting `guard`, leave the controllers of `guard` themselves empty. Do not attach direct controller permissions for the SNS root to the Canister targeting `guard`.
- guard only accepts reservation and cancellation requests from SNS governance principal. It does not disclose the target ID, WASM hash, argument hash, executable time, state, and customer information. Only 7 days after the reservation is confirmed, it allows an upgrade that matches the reservation content. The execution trigger can be anyone, but content changes are not allowed.
- We do not provide any APIs for arbitrary management calls, controller addition/transfer, reinstall, deletion, single stop, or short-term suspension. Emergency operational permissions are only for accepting new applications and stopping new orders, and arbitrary transfer, immediate upgrade, and change of withdrawal destination are not allowed. Normalization will be recorded via SNS.
- We will test the fixed settings of guard, the SNS generic function integration, cycle replenishment, and the behavior during upgrade in the test environment first. We do not assume that this controller configuration can be automatically obtained just by the SNS standard launch. The irreversible removal of the controller in the actual production will be carried out after audit and separate approval.
- Withdrawal is an opportunity for 7 days, but it is not a guarantee of fund recovery. You may not be able to withdraw due to HL suspension, unresolved transfers, collateral margin, or canister bugs. Malicious upgrades after the delay may access remaining funds and stored information. We do not display that the past confidentiality can be protected by a timelock.
- The flaws of guard itself cannot be corrected with upgrade. This recovery restriction will be audited. In the development environment, developers can leave the development controller, but they do not claim the same security as the real one. The token allocation and sales conditions of SNS are not determined by imagination, but are retained as the conditions for the start of the actual operation.

### 16.4 Initial trading and operational fixed values (U9/U10/U11/U13/U26)

- The initial allowlist consists only of BTC and ETH perps in HL standard. HIP-3, spot, portfolio margin, and unified account are not included in the initial scope. Standard account mode and separate margin are set as initial values, with leverage at the standard 3x and UI limit at 5x. This is a restriction for development purposes and is not a recommendation for safe investment multipliers.
- Market offers IOC limit orders with a slippage limit, with a fixed tolerance range of 0.5%. Limits are GTC, SL/TP are HL per position `positionTpsl`, and reduce-only. Complex brackets, custom bots, and TWAP are not eligible for initial entry. Margin changes after fund allocation are also authorized as master operations.
- The dead-man's switch is set to OFF. In consideration of the possibility that all order cancellations will be deleted up to the protection SL/TP, we display the suspension, unfilled order cancellation, and position settlement as separate operations. Emergency suspension does not automatically execute the order.
- The builder fee for testnet is 0. The actual fee, collection address, upper limit consent, and calculation are considered business decisions before the mainnet launch and are not collected implicitly.
- When an issue occurs, we prioritize stopping the increase of new risks, continuing reconciliation, and allowing cancellation, reduce-only, and confirmed withdrawals where possible. We design the system so that alternative minimum clients can request authentication, cancellation, and withdrawal, but we do not consider this an alternative execution basis for the Canister stop.
- Cycles are replenished from the operating budget rather than customer funds. The target is to cover 30 days of actual consumption, with notifications for 7 days and a mandatory suspension of new deposits and new risk acceptance for 3 days. Withdrawal and reconciliation amounts are secured separately. The funding recovery deadline is not promised without measuring actual consumption and HL restrictions.

### 16.5 Encryption, client path, storage (U4/U5/U12/U25)

- The initial mode is RFC 9180 HPKE (X25519 / HKDF-SHA256 / ChaCha20-Poly1305). Choose an implementation with audited experience and do not create cryptographic primitives. Because it is decrypted in the Canister inside the TEE, trust in the Canister code and the TEE itself remains.
- The client verifies HPKE public key, key ID, and expiration date using an authentication response from the IC. It protects both the request and response directions in the browser, and encrypts the response to the browser public key bound to the request. It includes network, canister, method, caller, request ID, and expiration date in the authentication target, and refuses to reuse them in different environments and purposes. HPKE itself is not a substitute for person authentication or resend prevention.
- HL direct connection in the browser is only based on the public market status. User-specific WS/REST, agent approval, and trading account reconciliation are centralized on the Canister side, and the connection IP and trading account are not directly transmitted via browser communication. HL itself knows the account and orders, and the gateway can observe the connection source IP and time.
- The initial distribution of personal data is done through encrypted polling. The browser reads Canister cache with an estimated 2 seconds of operation and 30 seconds of visibility. HLreconciliation is controlled by a separate shared budget and does not query HL for every browser polling. It stops the increase in new risks for accounts that have been in a state over 10 seconds old. It narrows the simultaneous usage limit when reconciliation capabilities are insufficient.
- Development eligibility is a composite attribute with the test issuer. The real external issuer registers after contract and legal verification. The mock token tests the separation of network, key, and build settings that do not work in the real network.
- Development data is only synthetic data. Signature payloads that have been terminal reconciliation are deleted after 24 hours, and detailed histories are deleted after 30 days. However, records required to prevent double processing, such as unknown, balance, unreleased reservations, current authentication and Agent status, are not deleted. The actual storage period and deletion basis require legal approval. We do not guarantee that deletion in the database will erase past snapshots or replica storage.

### 16.6 Fixed procedure for privacy verification (U29)

The basic path B is the starting point for safety verification and is not a completion method that hides the amount and time. In particular, "allocating the same amount immediately after deposit" is left as a failure counterpoint. By hiding this point, we do not call the prototype a privacy product.

1. Public observers are the primary evaluators. All publicly available transfer graphs, HL's publicly available account information, amount, fees, time, repetitive trading, and PnL are used. The observation scope of HL, entry operators, and TEE/SNS permission holders is evaluated separately in the table.
2. Compare A (direct deposit), B0 (immediate allocation equal to the amount via common custody), and B1 (simulation of attempting to separate and consolidate allocation timing) with the same input. In B1, it does not lend others' funds without permission and always maintains balance restrictions and backing. The allocation algorithm adopted is determined by the results, and it does not fix untested methods into the live specifications.
3. 1/5/20/100 users, generate 30 days worth of repetitive entry/withdrawal, small amounts, characteristic amounts, partial withdrawal, full withdrawal, and profit/loss using fixed seeds. Separate them into attack adjustment and unseen evaluation, and do not pass the correct answer response table to the attack input. Add public information not in the simulation through actual testnet bidirectional observation.
4. The development success criterion is set as a top-1 assignment success rate of less than 20% in each evaluation group with more than 20 people, a success rate reduction of more than 80% compared to A, and a proportion of less than 5% that can be directly identified. It is reported not only on the average but also by scenario and including repetitive history. The uncertainty interval for individual users is also reported, and the accuracy is not inflated by treating repetitive events as independent samples.
5. This number is a technical screening standard and does not guarantee mathematical anonymity. It does not guarantee it for individual or low-usage scenarios. We do not initiate new confidential deposits until the actual usage volume does not increase the water supply for synthetic users, and we do not start new confidential deposits until conditions are met. We do not allow existing funds to be withdrawn indefinitely until anonymity is achieved. If the protection weakens during withdrawal, we will notify you before proceeding.

Whether the test can be conducted in real-world conditions requires an independent review that includes the final algorithm, the actual observation range, and low-utilization conditions. If the criterion is not met, the fund transfer design for B will be reviewed, and unauthorized aggregation of trading accounts or switching to Agent-only mode will not be permitted.

### 16.7 Order of implementation first

1. Pure signature, authentication, integer ledger and a reverse-funding state machine with mock HL.
2. Failed double execution, unknown, upgrade, authorization, guard bypass tests in PocketIC.
3. Verify master/Agent approval, USDC transfers, order cancellation, reconciliation, and costs on the HL testnet.
4. Public trace evaluation of A/B0/B1. If privacynot met, redesign the funding pathway.
5. Connect the UI to the approved path and extend from a single user to multiple users.

Detailed Candid, DDL, and screen layouts will be implemented within this boundary. We do not guarantee the actual delivery date, cost, or anonymity before Phase 1 results are available.

## Change history

| Edition | Date | change |
|---|---|---|
| v0.1 | 2026-09-18 | First edition. Determine D1 to D8 and organize requirements, permission models, architecture, roadmap, and unresolved issues. |
| v0.2 | 2026-09-18 | Correct the impact evaluation of D3. Based on actual measurements (`meta` / `metaAndAssetCtxs`), add 8.5.4 (invariant condition of asset index) and 6.3.1 (fluency criterion), and revise the basis for Phase 3 work items and periods. |
| v0.3 | 2026-09-18 | Fix 8.3.2. Due to the need for temporary persistence of order parameters after separation of reception and signature (`Implementation.md` 2.3), specify the scope of storage and deletion conditions. Add 0.5 and set the record location for decision-making on implementation policy (D9~D12). |
| v0.4 | 2026-09-18 | Reconsider D1 and record D13/D14. Based on NEAR official specifications, separate the confidential fund layer and HL accounts. Explicitly define agent-only as the existing baseline, and add U23–U25 unresolved custody, withdrawal, and recovery matters, along with a pre-adoption gate. Distinguish between public links, amount correlation, and the scope of protection for signatures and communication. |
| v0.5 | 2026-09-18 | Corrected the design of fund privacy and signature pathway separation, Agent generation, action/order state, pre-transmission recording, fencing, deadlines, and recovery. Compatible with Implementation v0.3. |
| v0.6 | 2026-09-18 | Adopt confidential canister for customer fund management and SNS governance on D15/D16. Update the permission table, immutable conditions, deposits withdrawal, funds_vault, audit, and completion conditions for the B version. Prioritize accounts per HL user and do not combine single accounts. Add the risk of optional DAO updates, forced suspension, exit, recovery, fund separation, and U26–U28. Maintain the order safety correction in v0.5. Change the old delivery date to a reference value and explicitly disclose the misalignment in Implementation.md. |
| v0.7 | 2026-09-18 | Maintain confidential fund layer + SNS governance, and strengthen D13 to meet the actual requirements for open links and time correlation of amounts. Add D17 and 6.4.1/U29, and advance the A/B comparison of reverse fund flow to Phase 1. Clearly state that both fund safety and privacy improvement are conditions for live migration, and do not use NEAR's unconfirmed implementation as a qualification basis. |
| v0.8 | 2026-09-18 | In Chapter 16, master key, EOA authentication, USDC reverse transfer, guard, HPKE, account communication, initial market, and privacy evaluation are determined. Implementable items are separated from real-world, legal, and live gate systems, and Implementation v0.4 is integrated. |
| v0.9 | 2026-09-18 | Adopt Start+React, Workers as the main distribution, and Oxlint/Oxfmt, etc. Add 6 ADRs and a synthesized UI foundation. ICP backend, Candid, and connection points are unimplemented, and HPKE, authentication, and real trading are not connected yet. |
