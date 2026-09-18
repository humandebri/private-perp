# ADR-0003：SNSと変更不能guardによる7日猶予

- 日付：2026-09-18
- 設計状態：accepted
- 実証状態：未実装・未実証。UIデモは資金・暗号化・ガバナンスの証明ではない

## 背景

SNS rootによる直接upgradeは、可決直後に資金処理や機密情報の扱いを変えられる。

## 比較した案

SNS root直接制御、更新可能Canister内の待機時間、変更不能な外部guardを比較した。

## 決定

SNS governanceからguardへ変更を予約し、予約確定から7日後にだけhashが一致するupgradeを許可する。本番目標はguardを資金系Canisterの唯一のcontrollerにし、guard自身のcontrollersを空にする構成。任意management call、reinstall、controller追加、単独stop/delete、猶予短縮を提供しない。

## 欠点・残存リスク

guard自体のバグをupgradeで直せない。7日猶予は退出の機会であり回収保証ではない。猶予後の悪意ある変更は残存資金・保存情報へアクセスし得る。フロントエンド配信にはこの猶予が適用されない。

## 再検討条件

SNS連携・迂回拒否・cycles補充・退出演習が成立しない場合。本番controller除去は監査後の別途承認を要する。

## 検証の正

実行結果・未実装範囲はfrontend/README.mdとdocs/implementation-status.mdで追跡する。既存のPlan.md 16章の本番ゲートを省略しない。

