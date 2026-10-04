# Shared reserve account and individual trading account

Update: 2026-09-27. Destructive changes assuming no deployment yet. We will not implement migration from the old schema and old journal format. The requirement to maintain balance and the corresponding table by performing regular upgrade and snapshot restoration of the same version will remain.

## Implementation

- The reserve of `custody_accounts` is one account that does not belong to the user. `user_id` is NULL, reserve has a unique constraint and ownership constraints based on the type. trading account is an independent master account for the individual as usual. Each vault belongs to one HL network.
- Deposit information is returned to a common address and to the authentication-verified EOA HL address that should be used as the transfer source. Even if the external deposit address is common, internal balances, reservations, in transit funds, and trading equity are held separately by the individual.
- Only the person will be credited if the destination of HL's `internalTransfer` is verified and the transfer source and registered EOA match. The person is not identified only by the declaration of the amount, time, and public tx hash. Bridge deposits with unknown transfer sources or unregistered transfer sources will be placed in the unassigned account. The existing transfer API for controllers will remain as a trust boundary for management permissions.
- Recovery from a managed trading account to a shared reserve account will not be credited to a new deposit. It will be reflected in the settlement processing of the owner's recovery action. The elimination of duplicate external events will continue.
- In the journal account events, reserve owners are set to None, and in the deposit events, include the proof of transfer. The same deposit attribution logic is used for snapshot restoration. Account creation is reconfirmed in the writer fence.
- The existing nonce series of reserve will be used as the series of common signature accounts. The individual's fund reservation, signature withdrawal intent, and recovery fence will be maintained. No trading will be conducted on the common account, and no proxy transactions of others' balances will be carried out.

## Local screen

Normal operations are "deposit to LOCAL MOCK custody balance", "use custody balance for trading", and "withdrawal with MetaMask signature". Deposit only waits for the reflection on custody balance to be completed and does not allocate automatically. Users specify the required amount later and allocate it, then confirm the reflection on the trading account and complete it. No custody expiration period or forced waiting for anonymity is set. Withdrawal uses the available balance of the user's shared reserve account first, recovers only the insufficient amount from the trading account, requests withdrawal signature after the recovery fence is lifted and the reflection of the withdrawable balance. Allocation and recovery are not performed for each order.

This automation is a browser operation procedure and is not a new permanent transfer worker. Subsequent operations will stop if re-reading, logging out, timeout, or unknown outcome occur. Accepted fund processing will continue in the existing Canister's outbox, and the next time will be to re-read the balance and fence. If verification cannot be completed after 45 reading polling attempts, the waiting will be terminated, and the transfer will not be automatically resended with a different ID. Any recovery operations will be left to "balance adjustment".

mock HL also responds to unfilled order confirmation (`openOrders`) before recovery. It carries the transfer source in the deposit seed, and in `usdSend` it recovers the transfer source from the signature and returns the `internalTransfer` history. Before launching mock, it also builds auxiliary binaries with `cargo build -p e2e-signer`.

## Scope of privacy

Commonization serves as the foundation to avoid direct transfer pathways that correspond the individual to the trading account. Batches based on time windows, changes in amount units, and anonymity person-count determination are not yet implemented. It does not intentionally delay allocation, and the current method is close to the B0 of correlation evaluation. In the existing composite evaluation, B0 is not met, and this change does not constitute a pass for anonymity.

Real HL withdrawal and re-withdrawal, the completeness of transactions exceeding the acquisition limit at the same time, and real rate restrictions and cycles fees due to concentration in common accounts require separate acceptance. The single-time acquisition of all transactions is not used for anonymity or proof of complete fund recovery. The existing testnet connection environment will not be changed, and this screen test will be conducted with an independent local Canister and mock HL.

## Verification target

- Two people deposit the same amount into the same common address, and separately account for it by transfer source.
- Double counting is not done for re-incorporation of the same event or re-declaration with change of transfer source.
- Do not automatically charge the deposit from an unknown transfer source or not registered to the person in charge.
- Do not account the recovery from the trading account as a new deposit.
- Check account separation, signature-signed withdrawal, binding in case of unknown outcome, journal suspension, and snapshot restoration in the existing PocketIC group.
- The browser's automatic recovery only recovers the insufficient amount, does not resend existing recovery waits, and stops subsequent transfers when timeouts or unknown errors occur.

## Execution results

- All 37 integrated test files for PocketIC were successfully completed (136 files were completed) (corrected a mistake in the ID/address of the additional test files in the middle and re-executed files that failed). Includes a mixed load test with 20 and 100 participants. After the final depositwriter fence correction, rebuilt the 4 deposit and reconciliation files and re-verified them.
- 38 successful front-only items, 7 successful mock HL items, and 3 successful privacy evaluation tools.
- Rust clippy (Wasm, warnings are treated as errors), format, signature boundaries, await inspection during DB transactions, front-type inspection and linting are successful.
- Re-execute the existing synthetic privacy evaluation. B0, which passes through the common account immediately and at the same amount, has a 100% top-1 matching rate, but it does not meet the secrecy requirement.

- The screen's funding page has no suspicion in the 3 viewport integrity, horizontal overflow, or interaction inspection. There is only one warning indicating the presence of wiring in the handler inspection, which was supplemented by the interaction inspection and Playwright operations. The allocated screen after login is also confirmed by screenshots.
- Out of the 3 long browser E2E orders including existing orders, 3 were successful and 1 failed. In the new environment, deposits, allocation, and market orders were successful, but subsequent limit orders were waiting for result reconciliation. In the re-executed environment, the funds were stopped from subsequent operations due to unknown. It does not mean all browser E2E orders were successful.
- The standalone Playwright test for the funding path was successful (independent local Canister + mock, approximately 1.4 minutes). 100 USDC deposit → personal trading balance of 100 → recovery of the shortfall of 5 USDC → both withdrawal and recovery were settled → trading balance of 95 was confirmed. The verification fixture is `target/shared-reserve-e2e/frontend/tests/e2e/funds-flow.spec.ts`, and the log is `target/shared-reserve-e2e/funds-check-retry.log`. We completed the missing `openOrders` in the mock found in the process, and verified the existence of unfilled orders or empty arrays after cancellation using the mock test alone.

## Review correction (2026-09-28)

depositreconciliation retrieves one page per round using a network and account-specific permanent time-point cursor. It re-retries the last time point inclusive, and removes duplicates by event ID without dropping other deposits at the same time. After the total sum of all entries is successful, it proceeds the cursor sequentially, and if retrieval or journaling fails, it retries the same page. Manual reconciliation uses the same processing. If more than 500 entries are not returned only for the same time point, the cursor is retained as an error requiring complete evidence, and the time point is added without skipping.

After withdrawingE2E approves all cancellations and confirms that there are no unfilled orders on the venue, it will settle the positions 100%. After the venue's positions are zero, it will proceed to automatic recovery and withdrawal.

Verification: 16 deposit, reconciliation, and recovery operations in PocketIC were successful. Additional tests included 500 operations at the boundary of reconciliation, concurrent separate deposits, duplicates, cursor retention after journal failures, and rejection of saturation at the same time. Furthermore, cursor retention and ledger reconstruction from snapshots after Wasm upgrade were verified. Mock tests for 8 operations, 38 individual frontend operations, type checks, linting, Wasm clippy, signature boundary checks, and await checks were also successful. Overall Playwright in this independent local environment was successful in 3 cases and failed in 1 case. It stopped with "result reconciliation in progress" after allocation before the corrected withdrawal procedure (`target/review-fixes-e2e/browser.log`). The success of overall E2E, including the modified withdrawal procedure, is not confirmed.

## Balance maintenance operation (2026-09-28)

Remove automatic allocation from the usual deposit and complete it by reflecting the person's balance on the shared reserve account. Make "using custody balance for trading" an independent normal operation and allocate only the specified amount later. Maintain the recovery of the shortfall at the time of withdrawal. The custody balance is stored not in the browser's memory but in the Canister's ledger.

40 individual type inspection, lint, and frontend tests were successful. The additional gateway test verified that allocation is not done via deposit, and that only 20 allocations are made later to avoid creating a new deposit. The custody E2E between real Canister and mock was successful, confirming "custody 100 and trading 0" after 100 deposits, and the screen-to-screen retention after the exchange. The test that includes up to 20 real allocations was halted with the known "result reconciliation in progress," and the completion of arrival to the trading account is not confirmed. The integrity and scroll inspection by vlmkit 0.22.0 on the screen after logged in was without suspicion (there is an existing contrast warning for the small USDC characters).

## Correction of the overall review (2026-09-28)

- Save the POST responses for allocation and withdrawal to the local permanent queue, and retry only the recording and reconciliation to the independent journal. Do not resend POSTs to the trading platform. The reconciliation processing has been unified with snapshot replay.
- The loss of response for allocation and withdrawal is also considered for the reconciliation of fund history. We verify evidence that the sender, recipient, amount, and time range match. We do not eliminate competing transfers of the same amount or multiple matching histories by inference. Confirmation of unexecuted transactions requires the traditional history completeness setting and the expiration of the nonce acceptance deadline. Allocation deposits are not included before recording the results.
- Include the same cloid in the order JSON as the signing payload. Query orders with unknown oids using cloid and normalize the orderStatus that has become a nested order in the real API. Record the confirmed oid and order status in the journal before proceeding to fill and cancellation.
- Before the recovery process and during regular reconciliation, observe the HL accountValue and record the difference between the trader's trading assets and liabilities. The balance will include profits, losses, fees, and funding. If the ledger changes during the in transit of funds or while observing, no observation will be applied. Observation records can be played back from the independent journal.
- Direct deposits to a trading account where balance observation has been started are reflected in the following absolute balance observation. This is a handling to avoid double counting by adding the delayed history. The normal deposit and allocation deposit confirmation processing for shared reserve accounts continue. The variable response time of HL is excluded from the comparison object of replicated outcalls.
- Convert the fillfee from a decimal string to a signed micro-USDC. Also retain the maker rebate and do not replace missing or incorrect values with 0. Provide private Candid codec, display, mock, and fixture.

Even with this modification, the binding of transfers with ambiguous evidence will be maintained. The real HL's fund transfers and multiple node HTTPS response agreement are a separate acceptance item from verification by PocketIC and mock.

Screen funding polling waits for Dispatching and stops subsequent operations in Unknown. I also fixed the behavior of interrupting the wait during normal transmission.

Final verification: 56 related PocketIC files (core_pipeline / send_journal / vault_deposits / vault_outbox / vault_reconcile / vault_recovery), 41 front-end individual files, and 8 mock HL files were successful. Type checking, linting, Wasm clippy, Rust format, signature boundaries, and await checks during DB transactions were also successful. Candid and the front-end's generated bindings were updated. Logs are in `target/review-fix-final-tests.log`. Real HLtransfer and comprehensive browser E2E tests were not executed this time.
