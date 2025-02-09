use std::{fs, path::PathBuf};

use quote::quote;
use syn::{punctuated::Punctuated, Item, Meta, Signature, Token};

fn main() {
    if cfg!(feature = "wrapper") {
        println!("cargo::rerun-if-changed=src/wrapper/c.rs");
        multithread_codegen("src/wrapper/c.rs", "wrapper_multi.rs");
    } else if cfg!(feature = "shim") {
        println!("cargo::rerun-if-changed=src/shim/vfs/sahpool.rs");
        multithread_codegen("src/shim/vfs/sahpool.rs", "shim_multi_sahpool.rs");

        println!("cargo::rerun-if-changed=src/source");
        let path = std::env::current_dir().unwrap().join("source");
        let lib_path = path.to_str().unwrap();

        println!("cargo:rustc-link-search=native={lib_path}");
        if cfg!(feature = "custom-libc") {
            println!("cargo:rustc-link-lib=static=sqlite3");
        } else {
            println!("cargo:rustc-link-lib=static=sqlite3linked");
        }
    }
}

fn multithread_codegen(path: &str, out_name: &str) {
    let signatures = parse_fn(path);
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
    let path = PathBuf::new().join(output).join(out_name);
    fs::write(path, rs).expect("write multithreading failed");
}

/// Collect function signatures marked with the `multithread` attribute
fn parse_fn(path: &str) -> Vec<Signature> {
    let file = fs::read_to_string(path).unwrap();
    let ast = syn::parse_file(&file).unwrap();
    let mut result = Vec::with_capacity(ast.items.len());
    for item in ast.items.into_iter() {
        if let Item::Fn(f) = item {
            if f.attrs.iter().any(|x| {
                x.path().is_ident("cfg_attr") && {
                    let nested = x
                        .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                        .unwrap();
                    nested.iter().any(|x| {
                        x.path().is_ident("multithread") || x.path().is_ident("multithread_v2")
                    })
                }
            }) {
                result.push(f.sig);
            }
        }
    }
    result
}
