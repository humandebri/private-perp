# ローカルM5/M6検証記録

- 対象: loopback ICP replica、合成USDC、決定的mock HL。testnet・実資金の性能値ではない。
- 取引: Market建玉、Limit取消、SL/TP、部分・全決済、Cancel Allを実Canister経由で検証する。
- 障害: mockの一度きりscenarioでpartial/reject/unknown、info停止でstaleを再現する。unknown要求は自動再送しない。
- 分離: 公開市況WSにはEOA・取引口座を送らず、本人状態はHPKE封筒のCanister APIだけから取得する。
- UX目標: 注文クリック直後のローカルpending表示は100ms以内。CIでは機能回帰を合否条件とし、端末依存のp50/p95はtestnet計測時に追記する。
- 未測定: IC testnet署名時間、HL testnet受理時間、timer cycles、実HLレート制限。GATE 0後に記録する。
