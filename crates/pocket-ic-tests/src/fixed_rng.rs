//! 入力鍵材料から決定的な乱数列を作るRNG。
//!
//! canisterではOS乱数（`getrandom`）が使えないため、`raw_rand`の値を種にして
//! HPKEの暗号化乱数を供給する。`rand_core 0.10` では `TryRng`（`Error = Infallible`）を
//! 実装すると `CryptoRng` が自動で満たされる。

use core::convert::Infallible;
use rand_core::{TryCryptoRng, TryRng};

/// Keccakベースの決定的RNG。**テスト専用**（本番は`raw_rand`を種にする）。
pub struct FixedRng {
    seed: [u8; 32],
    counter: u64,
}

impl FixedRng {
    pub fn new(seed: [u8; 32]) -> Self {
        Self { seed, counter: 0 }
    }
}

impl TryRng for FixedRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut bytes = [0u8; 4];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut bytes = [0u8; 8];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        let mut offset = 0;
        while offset < dst.len() {
            let mut input = Vec::with_capacity(40);
            input.extend_from_slice(&self.seed);
            input.extend_from_slice(&self.counter.to_le_bytes());
            let block = hl_sign::keccak256(&input);
            let take = (dst.len() - offset).min(block.len());
            dst[offset..offset + take].copy_from_slice(&block[..take]);
            offset += take;
            self.counter += 1;
        }
        Ok(())
    }
}

impl TryCryptoRng for FixedRng {}
