//! Single Canister testnet composition. Each subsystem owns a distinct stable
//! SQLite database while its public methods are exported with a role prefix.

#[ic_cdk::init]
fn init(administrator: candid::Principal) {
    if administrator == candid::Principal::anonymous()
        || administrator == candid::Principal::management_canister()
        || administrator == ic_cdk::api::canister_self()
    {
        ic_cdk::trap("a non-anonymous external administrator is required");
    }
    send_journal::embedded_init();
    policy::embedded_init();
    funds_vault::embedded_init();
    trading_core::embedded_init();
    bootstrap_links(administrator);
}

// The install argument names the application administrator. IC controllers
// retain platform upgrade authority, not implicit application management rights.
fn bootstrap_links(administrator: candid::Principal) {
    let id = ic_cdk::api::canister_self();
    let principal = id.as_slice();
    let now = ic_cdk::api::time() / 1_000_000;
    let result: Result<(), db::error::Error> = (|| {
        db::tx::with_scope(db::DbScope::Vault, || {
            db::tx::update(|c| {
                db::repo::vault_config::set_policy_principal(c, principal, now)?;
                db::repo::vault_config::set_core_principal(c, principal, now)?;
                db::repo::send_journal_client::set_principal(c, principal)
            })
        })?;
        db::tx::with_scope(db::DbScope::Core, || {
            db::tx::update(|c| {
                db::repo::core_config::set_vault_principal(c, principal)?;
                db::repo::core_config::set_policy_principal(c, principal)?;
                db::repo::send_journal_client::set_principal(c, principal)
            })
        })?;
        db::tx::with_scope(db::DbScope::Policy, || {
            db::tx::update(|c| {
                db::repo::policy::initialize_administrator(c, administrator.as_slice())?;
                // The shared budget authorizes the application principal once.
                db::repo::budget::register_worker(c, "vault", principal)
            })
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        ic_cdk::trap(format!("internal configuration failed: {error}"));
    }
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    send_journal::embedded_post_upgrade();
    policy::embedded_post_upgrade();
    funds_vault::embedded_post_upgrade();
    trading_core::embedded_post_upgrade();
}

#[unsafe(no_mangle)]
pub fn get_candid_pointer() -> *mut std::os::raw::c_char {
    std::ffi::CString::new(include_str!("../../../candid/private_perp.did"))
        .expect("embedded Candid must not contain NUL")
        .into_raw()
}

#[ic_cdk::query]
fn application_administrator() -> candid::Principal {
    db::tx::with_scope(db::DbScope::Policy, || {
        let bytes = db::tx::query(db::repo::policy::administrator)
            .expect("administrator storage")
            .expect("administrator configured at installation");
        candid::Principal::from_slice(&bytes)
    })
}
