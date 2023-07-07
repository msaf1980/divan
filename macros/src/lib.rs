//! Macros for [Divan](https://github.com/nvzqz/divan), a statistically-comfy
//! benchmarking library brought to you by [Nikolai Vazquez](https://hachyderm.io/@nikolai).
//!
//! See [`divan`](https://docs.rs/divan) crate for documentation.

use proc_macro::TokenStream;
use quote::{quote, ToTokens};

#[proc_macro_attribute]
pub fn bench(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut divan_crate = None::<syn::Path>;
    let mut bench_name_expr = None::<syn::Expr>;

    let attr_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("crate") {
            divan_crate = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("name") {
            bench_name_expr = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error("unsupported 'bench' property"))
        }
    });

    syn::parse_macro_input!(attr with attr_parser);

    // Items needed by generated code.
    //
    // Access to libstd is through a re-export because it's possible (although
    // unlikely) to do `extern crate x as std`, which would cause `::std` to
    // reference crate `x` instead.
    let divan_crate = divan_crate.unwrap_or_else(|| syn::parse_quote!(::divan));
    let private_mod = quote! { #divan_crate::__private };
    let linkme_crate = quote! { #private_mod::linkme };
    let std_crate = quote! { #private_mod::std };

    let fn_item = item.clone();
    let fn_item = syn::parse_macro_input!(fn_item as syn::ItemFn);
    let fn_name = &fn_item.sig.ident;

    let mut ignore = false;
    for attr in &fn_item.attrs {
        let path = attr.meta.path();

        if path.is_ident("ignore") {
            ignore = true;
            break;
        }
    }

    // String expression of the benchmark's fully-qualified path.
    let bench_path_expr = {
        let fn_name = fn_name.to_string();
        let fn_name = fn_name.strip_prefix("r#").unwrap_or(&fn_name);

        quote! { #std_crate::concat!(#std_crate::module_path!(), "::", #fn_name) }
    };

    let bench_name_expr: &dyn ToTokens = match &bench_name_expr {
        Some(name) => name,
        None => &bench_path_expr,
    };

    let fn_args = &fn_item.sig.inputs;

    let bench_loop = if fn_args.is_empty() {
        // `fn(&mut divan::bench::Context) -> ()`.
        quote! {
            #private_mod::BenchLoop::Static(|__divan_context| {
                __divan_context.bench_loop(#fn_name)
            })
        }
    } else {
        // `fn(divan::Bencher) -> ()`.
        quote! { #private_mod::BenchLoop::Runtime(#fn_name) }
    };

    let entry_item = quote! {
        // This `const _` prevents collisions in the current scope by giving us
        // an anonymous scope to place our static in. As a result, this macro
        // can be used multiple times within the same scope.
        #[doc(hidden)]
        const _: () = {
            #[#linkme_crate::distributed_slice(#private_mod::ENTRIES)]
            #[linkme(crate = #linkme_crate)]
            static __DIVAN_BENCH_ENTRY: #private_mod::Entry = #private_mod::Entry {
                name: #bench_name_expr,
                path: #bench_path_expr,

                // `Span` location info is nightly-only, so use macros.
                file: #std_crate::file!(),
                line: #std_crate::line!(),

                ignore: #ignore,

                bench_loop: #bench_loop,

                get_id: || #std_crate::any::Any::type_id(&#fn_name),
            };
        };
    };

    // Append our generated code to the existing token stream.
    let mut result = item;
    result.extend(TokenStream::from(entry_item));
    result
}
