//! Domain-separated request IDs for the shared REST budget.

#[derive(Clone, Copy)]
pub enum Worker {
    Vault,
    Core,
}

pub fn request_id(
    worker: Worker,
    canister: &[u8],
    time_ns: u64,
    attempt: u64,
    expires_at: u64,
) -> [u8; 32] {
    let role: &[u8] = match worker {
        Worker::Vault => b"vault",
        Worker::Core => b"core",
    };
    let mut id = crate::keccak256_concat(&[
        b"private-perp/rest-budget/v1",
        role,
        canister,
        &time_ns.to_be_bytes(),
        &attempt.to_be_bytes(),
    ]);
    id[..8].copy_from_slice(&expires_at.to_be_bytes());
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simultaneous_workers_have_distinct_ids_with_bound_expiry() {
        let expiry = 31_000u64;
        let vault = request_id(Worker::Vault, &[1, 2, 3], 1_000_000_000, 0, expiry);
        let core = request_id(Worker::Core, &[1, 2, 3], 1_000_000_000, 0, expiry);
        assert_ne!(vault, core);
        assert_eq!(&vault[..8], &expiry.to_be_bytes());
        assert_eq!(&core[..8], &expiry.to_be_bytes());
        assert_ne!(
            vault,
            request_id(Worker::Vault, &[1, 2, 3], 1_000_000_000, 1, expiry)
        );
    }
}
