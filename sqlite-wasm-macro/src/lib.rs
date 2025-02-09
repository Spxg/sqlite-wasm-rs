#![doc = include_str!("../README.md")]

extern crate proc_macro2;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_str, Expr, ItemFn, LitStr};

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
            let sqlite3 = sqlite();
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

#[proc_macro_attribute]
pub fn multithread_v2(input: TokenStream, item: TokenStream) -> TokenStream {
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

    let extra = parse_macro_input!(input as LitStr);
    let expr = parse_str::<Expr>(&extra.value()).unwrap();

    quote!(
        #(#attrs)*
        #vis #sig {
            let handle = get_handle(#expr);
            if !handle.main_thread() {
                let CApiResp::#ident(ret) = call(handle, CApiReq::#ident((#(#args),*))) else {
                    unreachable!()
                };
                return ret;
            }
            #block
        }
    )
    .into()
}
