//! Helps generate multithreaded code

use std::{fs, path::PathBuf};

use quote::quote;
use syn::{Item, Signature};

fn main() {
    println!("cargo::rerun-if-changed=src/c.rs");

    let signatures = parse_fn();
    let mut req = Vec::with_capacity(signatures.len());
    let mut resp = Vec::with_capacity(signatures.len());
    let mut call_pat = Vec::with_capacity(signatures.len());

    for sig in signatures {
        let ident = sig.ident;

        let mut args = Vec::with_capacity(sig.inputs.len());
        for input in sig.inputs.iter() {
            let syn::FnArg::Typed(pat_type) = input else {
                unreachable!()
            };
            args.push(&pat_type.ty);
        }
        req.push(quote! {
            #[allow(unused_parens)]
            #ident((#(#args),*))
        });

        let arg = match sig.output {
            syn::ReturnType::Default => quote! { () },
            syn::ReturnType::Type(_, output) => quote! { #output },
        };
        resp.push(quote! {
            #[allow(unused_parens)]
            #ident(#arg)
        });

        let p = (0..args.len())
            .map(|x| {
                let ident = syn::Ident::new(&format!("arg{x}"), ident.span());
                quote! { #ident }
            })
            .collect::<Vec<_>>();
        call_pat.push(quote! {
            #[allow(unused_parens)]
            CApiReq::#ident((#(#p),*)) => CApiResp::#ident(#ident(#(#p),*))
        });
    }

    let rs = syn::parse_quote! {
        #[allow(non_camel_case_types)]
        pub(crate) enum CApiReq {
            #(#req),*
        }

        impl CApiReq {
            pub fn call(self) -> CApiResp {
                unsafe {
                    match self {
                        #(#call_pat),*
                    }
                }
            }
        }

        #[allow(non_camel_case_types)]
        pub(crate) enum CApiResp {
            #(#resp),*
        }

        unsafe impl Send for CApiReq {}
        unsafe impl Sync for CApiReq {}

        unsafe impl Send for CApiResp {}
        unsafe impl Sync for CApiResp {}
    };

    let rs = prettyplease::unparse(&rs);
    let output = std::env::var("OUT_DIR").expect("OUT_DIR env not set");
    let path = PathBuf::new().join(output).join("multithreading.rs");
    fs::write(path, rs).expect("write multithreading failed");
}

/// Collect function signatures marked with the `multithread` attribute
fn parse_fn() -> Vec<Signature> {
    let file = fs::read_to_string("src/c.rs").unwrap();
    let ast = syn::parse_file(&file).unwrap();
    let mut result = Vec::with_capacity(ast.items.len());
    for item in ast.items.into_iter() {
        if let Item::Fn(f) = item {
            if f.attrs.iter().any(|x| x.path().is_ident("multithread")) {
                result.push(f.sig);
            }
        }
    }
    result
}
