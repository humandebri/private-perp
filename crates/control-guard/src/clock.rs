//! 時刻。テストではPocketICの `set_time`／`advance_time` で決定的にする。

/// 現在時刻（ミリ秒）。
pub fn now_ms() -> u64 {
    ic_cdk::api::time() / 1_000_000
}
