use proc_macro::TokenStream;
use quote::quote;
use syn::{Ident, ItemFn, Token, parse::Parse, parse_macro_input};

struct Args {
    scope: Ident,
    prefix: syn::LitStr,
}

impl Parse for Args {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let scope_key: Ident = input.parse()?;
        if scope_key != "scope" {
            return Err(syn::Error::new(scope_key.span(), "expected scope"));
        }
        input.parse::<Token![=]>()?;
        let scope = input.parse()?;
        input.parse::<Token![,]>()?;
        let prefix_key: Ident = input.parse()?;
        if prefix_key != "prefix" {
            return Err(syn::Error::new(prefix_key.span(), "expected prefix"));
        }
        input.parse::<Token![=]>()?;
        let prefix = input.parse()?;
        Ok(Self { scope, prefix })
    }
}

fn expand(kind: &str, args: TokenStream, item: TokenStream) -> TokenStream {
    let Args { scope, prefix } = parse_macro_input!(args as Args);
    let mut function = parse_macro_input!(item as ItemFn);
    let old_body = function.block;
    // Keep non-conflicting names stable for existing inter-module calls and
    // frontend bindings. Only shared names require a role prefix.
    let ident = function.sig.ident.to_string();
    let standalone_only = matches!(
        ident.as_str(),
        "set_vault_principal"
            | "set_core_principal"
            | "set_policy_principal"
            | "set_send_journal"
            | "set_journal_guard"
            | "get_journal_guard"
            | "set_sns_principal"
            | "set_operator"
            | "set_guard_principal"
            | "get_role_principal"
            | "register_budget_worker"
            | "register_worker"
    );
    let collision = matches!(
        ident.as_str(),
        "configure_cycles"
            | "configure_eligibility"
            | "configure_market_threshold"
            | "configure_rest_budget"
            | "get_cycles_status"
            | "get_environment"
            | "get_hpke_public_key"
            | "get_journal_guard"
            | "get_journal_send_status"
            | "get_policy_principal"
            | "get_send_journal"
            | "journal_restore_status"
            | "private_call"
            | "recovery_replay_pending"
            | "recovery_stage_status"
            | "resume_journal"
            | "rotate_hpke_key"
            | "set_ecdsa_key_id"
            | "set_journal_guard"
            | "set_policy_principal"
            | "set_send_journal"
            | "set_sns_principal"
            | "set_venue_endpoints"
            | "test_sweep_now"
            | "version"
    );
    let name = if collision {
        format!("{}{}", prefix.value(), ident)
    } else {
        ident
    };
    let renamed = syn::LitStr::new(&name, prefix.span());
    let attribute = match kind {
        "query" => quote! {
            #[cfg_attr(not(feature = "embedded"), ic_cdk::query)]
            #[cfg_attr(feature = "embedded", ic_cdk::query(name = #renamed))]
        },
        _ => quote! {
            #[cfg_attr(not(feature = "embedded"), ic_cdk::update)]
            #[cfg_attr(feature = "embedded", ic_cdk::update(name = #renamed))]
        },
    };
    let wrapped = if function.sig.asyncness.is_some() {
        quote! { db::tx::with_scope_future(db::DbScope::#scope, async move #old_body).await }
    } else {
        quote! { db::tx::with_scope(db::DbScope::#scope, || #old_body) }
    };
    function.block = Box::new(syn::parse_quote!({
        #[cfg(feature = "embedded")]
        { #wrapped }
        #[cfg(not(feature = "embedded"))]
        #old_body
    }));
    let enabled = standalone_only.then(|| quote! { #[cfg(not(feature = "embedded"))] });
    quote! { #enabled #attribute #function }.into()
}

#[proc_macro_attribute]
pub fn query(args: TokenStream, item: TokenStream) -> TokenStream {
    expand("query", args, item)
}

#[proc_macro_attribute]
pub fn update(args: TokenStream, item: TokenStream) -> TokenStream {
    expand("update", args, item)
}
