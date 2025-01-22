#![doc = include_str!("../README.md")]

extern crate proc_macro2;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn multithread(_: TokenStream, item: TokenStream) -> TokenStream {
    let ItemFn {
        sig,
        vis,
        block,
        attrs,
    } = parse_macro_input!(item as ItemFn);

    let ident = &sig.ident;
    let mut args = Vec::with_capacity(sig.inputs.len());
    for input in sig.inputs.iter() {
        let syn::FnArg::Typed(pat_type) = input else {
            unreachable!()
        };
        args.push(&pat_type.pat);
    }

    quote!(
        #(#attrs)*
        #vis #sig {
            #[cfg(target_feature = "atomics")]
            let sqlite3 = sqlite();
            #[cfg(target_feature = "atomics")]
            if !sqlite3.main_thread() {
                let CApiResp::#ident(ret) = call(sqlite3, CApiReq::#ident((#(#args),*))) else {
                    unreachable!()
                };
                return ret;
            }
            #block
        }
    )
    .into()
}
