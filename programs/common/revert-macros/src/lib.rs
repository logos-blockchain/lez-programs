//! Metered guest entry points for the pinned SPEL framework.
//!
//! SPEL still panics when dispatch or account validation fails. This adapter
//! derives dispatch from the same instruction signatures and calls SPEL's
//! generated validators and handlers (including its automatic claims). Remove
//! the adapter when SPEL supports nonzero exits at those boundaries.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, Item, ItemFn, ItemMod, Pat, Path, Type};

/// Add `metered_main` before expanding `#[lez_program]`.
///
/// The argument names the program-local rejection code. Guest binaries must
/// select `entry!(metered_main)`. Instruction layouts and IDL remain owned by
/// SPEL. Only the fixed-account signatures used by this repository are supported;
/// unsupported constraints fail compilation rather than skipping validation.
#[proc_macro_attribute]
pub fn metered_entry(code: TokenStream, input: TokenStream) -> TokenStream {
    let code = parse_macro_input!(code as Path);
    let module = parse_macro_input!(input as ItemMod);
    match expand(&code, &module) {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand(code: &Path, module: &ItemMod) -> syn::Result<Tokens> {
    let (_, items) = module.content.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(
            module,
            "metered_entry requires an inline lez_program module",
        )
    })?;
    let mut arms = Vec::new();
    for item in items {
        if let Item::Fn(handler) = item {
            if handler
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("instruction"))
            {
                arms.push(dispatch_arm(code, &module.ident, handler)?);
            }
        }
    }
    if arms.is_empty() {
        return Err(syn::Error::new_spanned(
            module,
            "no instructions for metered_entry",
        ));
    }

    Ok(quote! {
        #module

        #[cfg(not(test))]
        pub fn metered_main() {
            use program_revert::UnwrapOrRevert as _;
            // The host owns the input framing; instruction words are caller
            // supplied and must be decoded through the expected-error path.
            let self_program_id: nssa_core::program::ProgramId = risc0_zkvm::guest::env::read();
            let caller_program_id: Option<nssa_core::program::ProgramId> = risc0_zkvm::guest::env::read();
            let pre_states: Vec<nssa_core::account::AccountWithMetadata> = risc0_zkvm::guest::env::read();
            let instruction_words: nssa_core::program::InstructionData = risc0_zkvm::guest::env::read();
            let instruction: Instruction = program_revert::decode_instruction(&instruction_words)
                .unwrap_or_revert(#code, "Invalid instruction data");
            let pre_states_clone = pre_states.clone();
            let output = match instruction { #(#arms)* };
            let parts = output
                .unwrap_or_revert(#code, "Program error")
                .into_parts();

            // Match SPEL's output contract: omit unclaimed foreign signer
            // accounts whose non-default state cannot be returned to LEZ.
            let (filtered_pre, filtered_post): (Vec<_>, Vec<_>) = pre_states_clone
                .into_iter()
                .zip(parts.post_states)
                .filter(|(pre, post)| {
                    pre.account.program_owner != nssa_core::program::DEFAULT_PROGRAM_ID
                        || pre.account == nssa_core::account::Account::default()
                        || post.required_claim().is_some()
                })
                .unzip();
            nssa_core::program::ProgramOutput::new(
                self_program_id, caller_program_id, instruction_words,
                filtered_pre, filtered_post,
            )
            .with_chained_calls(parts.chained_calls)
            .with_block_validity_window(parts.block_validity_window)
            .with_timestamp_validity_window(parts.timestamp_validity_window)
            .write();
        }
    })
}

fn dispatch_arm(code: &Path, module: &syn::Ident, handler: &ItemFn) -> syn::Result<Tokens> {
    let name = &handler.sig.ident;
    let variant = name
        .to_string()
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<String>();
    let variant = format_ident!("{variant}");
    let mut accounts = Vec::new();
    let mut arguments = Vec::new();
    let mut call_arguments = Vec::new();
    let mut needs_validation = false;
    for input in &handler.sig.inputs {
        let FnArg::Typed(input) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "instruction cannot take self",
            ));
        };
        let Pat::Ident(binding) = &*input.pat else {
            return Err(syn::Error::new_spanned(
                input,
                "instruction requires named parameters",
            ));
        };
        let ident = &binding.ident;
        let Type::Path(ty) = &*input.ty else {
            return Err(syn::Error::new_spanned(
                input,
                "unsupported metered parameter type",
            ));
        };
        let last = ty
            .path
            .segments
            .last()
            .ok_or_else(|| syn::Error::new_spanned(ty, "missing parameter type"))?;
        if last.ident == "ProgramContext" {
            call_arguments.push(quote! {
                spel_framework::context::ProgramContext::new(
                    self_program_id,
                    caller_program_id.unwrap_or(nssa_core::program::DEFAULT_PROGRAM_ID),
                )
            });
        } else {
            call_arguments.push(quote! { #ident });
            if last.ident == "AccountWithMetadata" {
                accounts.push(ident);
            } else {
                if quote!(#ty).to_string().contains("AccountWithMetadata") {
                    return Err(syn::Error::new_spanned(
                        input,
                        "metered_entry requires fixed accounts",
                    ));
                }
                arguments.push(ident);
            }
        }
        for attr in input
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("account"))
        {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("signer") || meta.path.is_ident("init") {
                    needs_validation = true;
                } else if meta.path.is_ident("owner") {
                    let _: syn::Expr = meta.value()?.parse()?;
                    needs_validation = true;
                } else if !meta.path.is_ident("mut") {
                    return Err(
                        meta.error("extend metered_entry dispatch for this account constraint")
                    );
                }
                Ok(())
            })?;
        }
    }
    let pattern = if arguments.is_empty() {
        quote! { Instruction::#variant }
    } else {
        quote! { Instruction::#variant { #(#arguments),* } }
    };
    let validate = if needs_validation {
        let validator = format_ident!("__validate_{name}");
        quote! {
            #module::#validator(&pre_states, &self_program_id, &instruction_words)
                .unwrap_or_revert(#code, "account validation failed");
        }
    } else {
        quote! {}
    };
    let count = accounts.len();
    Ok(quote! {
        #pattern => {
            program_revert::require_eq!(
                #code, pre_states.len(), #count, "Account count mismatch"
            );
            #validate
            let [#(#accounts),*] = <[_; #count]>::try_from(pre_states)
                .expect("account count was validated");
            #module::#name(#(#call_arguments),*)
        },
    })
}
