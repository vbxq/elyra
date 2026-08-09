// generates extern "C" wrappers for #[aelys_export] fns

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ItemFn, Pat, PatType, ReturnType, Type};

pub struct ExportInfo {
    pub name: String,
    pub arity: u16,
    pub wrapper_name: Ident,
}

pub fn generate_export_wrapper(
    func: &ItemFn,
    module_prefix: &str,
    module_ident: &Ident,
) -> syn::Result<(TokenStream2, ExportInfo)> {
    let fn_name = &func.sig.ident;
    let wrapper_name = format_ident!("__aelys_wrapper_{}_{}", module_prefix, fn_name);
    let export_symbol = format!("aelys_native_{}_{}", module_prefix, fn_name);

    if func.sig.abi.is_some() {
        let arity = if func.sig.inputs.len() >= 4 {
            0
        } else {
            u16::try_from(func.sig.inputs.len())
                .map_err(|_| syn::Error::new(func.sig.inputs.span(), "too many parameters"))?
        };

        let wrapper = quote! {
            #[doc(hidden)]
            #[unsafe(export_name = #export_symbol)]
            pub use #module_ident::#fn_name as #wrapper_name;
        };

        return Ok((
            wrapper,
            ExportInfo {
                name: fn_name.to_string(),
                arity,
                wrapper_name: fn_name.clone(),
            },
        ));
    }

    let mut param_extractions = Vec::new();
    let mut call_args = Vec::new();
    let mut arity: u16 = 0;

    for (i, arg) in func.sig.inputs.iter().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let param_name = match &**pat {
                Pat::Ident(ident) => &ident.ident,
                _ => return Err(syn::Error::new(pat.span(), "expected identifier pattern")),
            };

            let extraction = generate_extraction(param_name, ty, i)?;
            param_extractions.push(extraction);
            call_args.push(quote! { #param_name });
            arity = arity
                .checked_add(1)
                .ok_or_else(|| syn::Error::new(arg.span(), "too many parameters"))?;
        }
    }

    let return_conversion = match &func.sig.output {
        ReturnType::Default => quote! { Ok(::aelys_native::value_null()) },
        ReturnType::Type(_, ty) => generate_return_conversion(ty)?,
    };

    let wrapper = quote! {
        #[doc(hidden)]
        #[unsafe(export_name = #export_symbol)]
        pub unsafe extern "C" fn #wrapper_name(
            _context: *mut ::aelys_native::NativeContext,
            args: *const ::aelys_native::AelysValue,
            _arg_count: usize,
            out: *mut ::aelys_native::AelysValue,
        ) -> i32 {
            if out.is_null()
                || _arg_count != usize::from(#arity)
                || (_arg_count != 0 && args.is_null())
            {
                return ::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
            }
            let result: Result<::aelys_native::AelysValue, i32> = (|| {
                #(#param_extractions)*
                let ret = #module_ident::#fn_name(#(#call_args),*);
                #return_conversion
            })();

            let result = match result {
                Ok(value) => value,
                Err(status) => return status,
            };

            unsafe {
                *out = result;
            }
            0
        }
    };

    Ok((
        wrapper,
        ExportInfo {
            name: fn_name.to_string(),
            arity,
            wrapper_name,
        },
    ))
}

fn generate_extraction(name: &Ident, ty: &Type, index: usize) -> syn::Result<TokenStream2> {
    let ty_str = quote!(#ty).to_string().replace(' ', "");
    let idx = index;

    let extraction = match ty_str.as_str() {
        "i64" => quote! {
            if !unsafe { ::aelys_native::value_is_int(*args.add(#idx)) } {
                return Err(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT);
            }
            let #name: i64 = unsafe { ::aelys_native::value_as_int(*args.add(#idx)) };
        },
        "f64" => quote! {
            if !unsafe { ::aelys_native::value_is_float(*args.add(#idx)) } {
                return Err(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT);
            }
            let #name: f64 = unsafe { ::aelys_native::value_as_float(*args.add(#idx)) };
        },
        "bool" => quote! {
            if !unsafe { ::aelys_native::value_is_bool(*args.add(#idx)) } {
                return Err(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT);
            }
            let #name: bool = unsafe { ::aelys_native::value_as_bool(*args.add(#idx)) };
        },
        "String" => quote! {
            let #name: String = unsafe {
                ::aelys_native::read_string_from_value(_context, *args.add(#idx))
            }.ok_or(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT)?;
        },
        _ => {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "unsupported parameter type: {}. Supported: i64, f64, bool, String",
                    ty_str
                ),
            ));
        }
    };

    Ok(extraction)
}

fn generate_return_conversion(ty: &Type) -> syn::Result<TokenStream2> {
    let ty_str = quote!(#ty).to_string().replace(' ', "");

    let conversion = match ty_str.as_str() {
        "i64" => quote! {
            ::aelys_native::value_int(ret)
                .ok_or(::aelys_native::AELYS_NATIVE_INTEGER_OVERFLOW)
        },
        "f64" => quote! { Ok(::aelys_native::value_float(ret)) },
        "bool" => quote! { Ok(::aelys_native::value_bool(ret)) },
        "()" => quote! { { ret; Ok(::aelys_native::value_null()) } },
        _ => {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "unsupported return type: {}. Supported: i64, f64, bool, ()",
                    ty_str
                ),
            ));
        }
    };

    Ok(conversion)
}
