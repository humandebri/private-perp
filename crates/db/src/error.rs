//! `db` のエラー。
//!
//! SQL失敗とドメイン上の拒否を分けて扱い、呼び出し側（Canister）が
//! `docs/phase-0/api-contract.md` の `ErrorCode` へ写せるようにする。

/// 永続化層のエラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// SQLiteの失敗（制約違反・I/O・未初期化など）。
    Sql(String),
    /// 期待した行が無い。
    NotFound,
    /// 一意制約などによる競合（同一キーの異なる内容を含む）。
    Conflict,
    /// 残高・予約が不足している。
    InsufficientFunds { available: i64, requested: i64 },
    /// 状態遷移が許可されていない、またはCASに敗れた。
    StateConflict { expected: String, actual: String },
    /// 整数オーバーフロー。
    Overflow,
    /// 台帳の不変条件違反（仕訳の不均衡など）。
    Invariant(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(message) => write!(formatter, "sql error: {message}"),
            Self::NotFound => write!(formatter, "not found"),
            Self::Conflict => write!(formatter, "conflict"),
            Self::InsufficientFunds {
                available,
                requested,
            } => write!(formatter, "insufficient funds: {available} < {requested}"),
            Self::StateConflict { expected, actual } => {
                write!(
                    formatter,
                    "state conflict: expected {expected}, actual {actual}"
                )
            }
            Self::Overflow => write!(formatter, "integer overflow"),
            Self::Invariant(message) => write!(formatter, "invariant violated: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// SQLiteの制約違反を `Conflict` として扱うか判定する。
pub fn classify_sql(message: String) -> Error {
    let lowered = message.to_lowercase();
    if lowered.contains("unique constraint")
        || lowered.contains("primary key")
        || lowered.contains("constraint failed")
    {
        Error::Conflict
    } else {
        Error::Sql(message)
    }
}
