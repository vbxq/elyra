// generates extern "C" wrappers for #[aelys_export] fns

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ItemFn, Pat, PatType, ReturnType, Type};

#[derive(Clone, Copy)]
pub enum ExportType {
    Int,
    Float,
    Bool,
    String,
    Unit,
    OptionInt,
    OptionFloat,
    OptionBool,
    OptionString,
    OptionUnit,
    ResultIntString,
    ResultFloatString,
    ResultBoolString,
    ResultStringString,
    ResultUnitString,
}

impl ExportType {
    pub(crate) fn ffi_value(self) -> TokenStream2 {
        let variant = match self {
            Self::Int => quote! { Int },
            Self::Float => quote! { Float },
            Self::Bool => quote! { Bool },
            Self::String => quote! { String },
            Self::Unit => quote! { Unit },
            Self::OptionInt => quote! { OptionInt },
            Self::OptionFloat => quote! { OptionFloat },
            Self::OptionBool => quote! { OptionBool },
            Self::OptionString => quote! { OptionString },
            Self::OptionUnit => quote! { OptionUnit },
            Self::ResultIntString => quote! { ResultIntString },
            Self::ResultFloatString => quote! { ResultFloatString },
            Self::ResultBoolString => quote! { ResultBoolString },
            Self::ResultStringString => quote! { ResultStringString },
            Self::ResultUnitString => quote! { ResultUnitString },
        };
        quote! { ::aelys_native::AelysNativeType::#variant as u8 }
    }
}

pub struct ExportInfo {
    pub name: String,
    pub arity: u16,
    pub wrapper_name: Ident,
    pub signature: Option<(Vec<ExportType>, ExportType)>,
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
                signature: None,
            },
        ));
    }

    let mut param_extractions = Vec::new();
    let mut call_args = Vec::new();
    let mut param_types = Vec::new();
    let mut arity: u16 = 0;

    for (i, arg) in func.sig.inputs.iter().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let param_name = match &**pat {
                Pat::Ident(ident) => &ident.ident,
                _ => return Err(syn::Error::new(pat.span(), "expected identifier pattern")),
            };

            let (extraction, param_type) = generate_extraction(param_name, ty, i)?;
            param_extractions.push(extraction);
            param_types.push(param_type);
            call_args.push(quote! { #param_name });
            arity = arity
                .checked_add(1)
                .ok_or_else(|| syn::Error::new(arg.span(), "too many parameters"))?;
        }
    }

    let (return_conversion, return_type) = match &func.sig.output {
        ReturnType::Default => (
            quote! { Ok(::aelys_native::value_unit()) },
            ExportType::Unit,
        ),
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
            signature: Some((param_types, return_type)),
        },
    ))
}

fn generate_extraction(
    name: &Ident,
    ty: &Type,
    index: usize,
) -> syn::Result<(TokenStream2, ExportType)> {
    let ty_str = quote!(#ty).to_string().replace(' ', "");
    let idx = index;

    let (extraction, export_type) = match ty_str.as_str() {
        "i64" => (
            quote! {
                if !unsafe { ::aelys_native::value_is_int(*args.add(#idx)) } {
                    return Err(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT);
                }
                let #name: i64 = unsafe { ::aelys_native::value_as_int(*args.add(#idx)) };
            },
            ExportType::Int,
        ),
        "f64" => (
            quote! {
                if !unsafe { ::aelys_native::value_is_float(*args.add(#idx)) } {
                    return Err(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT);
                }
                let #name: f64 = unsafe { ::aelys_native::value_as_float(*args.add(#idx)) };
            },
            ExportType::Float,
        ),
        "bool" => (
            quote! {
                if !unsafe { ::aelys_native::value_is_bool(*args.add(#idx)) } {
                    return Err(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT);
                }
                let #name: bool = unsafe { ::aelys_native::value_as_bool(*args.add(#idx)) };
            },
            ExportType::Bool,
        ),
        "String" => (
            quote! {
                let #name: String = unsafe {
                    ::aelys_native::read_string_from_value(_context, *args.add(#idx))
                }.ok_or(::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT)?;
            },
            ExportType::String,
        ),
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

    Ok((extraction, export_type))
}

fn generate_return_conversion(ty: &Type) -> syn::Result<(TokenStream2, ExportType)> {
    let ty_str = quote!(#ty).to_string().replace(' ', "");

    if let Some(inner) = ty_str
        .strip_prefix("Option<")
        .and_then(|value| value.strip_suffix('>'))
    {
        let (conversion, _) = scalar_return_conversion(inner, quote! { value })?;
        let export_type = match inner {
            "i64" => ExportType::OptionInt,
            "f64" => ExportType::OptionFloat,
            "bool" => ExportType::OptionBool,
            "String" => ExportType::OptionString,
            "()" => ExportType::OptionUnit,
            _ => return unsupported_return_type(ty, &ty_str),
        };
        return Ok((
            quote! {
                match ret {
                    Some(value) => #conversion,
                    None => Err(::aelys_native::AELYS_NATIVE_OPTION_NONE),
                }
            },
            export_type,
        ));
    }

    if let Some(inner) = ty_str
        .strip_prefix("Result<")
        .and_then(|value| value.strip_suffix(",String>"))
    {
        let (conversion, _) = scalar_return_conversion(inner, quote! { value })?;
        let export_type = match inner {
            "i64" => ExportType::ResultIntString,
            "f64" => ExportType::ResultFloatString,
            "bool" => ExportType::ResultBoolString,
            "String" => ExportType::ResultStringString,
            "()" => ExportType::ResultUnitString,
            _ => return unsupported_return_type(ty, &ty_str),
        };
        return Ok((
            quote! {
                match ret {
                    Ok(value) => #conversion,
                    Err(error) => {
                        let message = unsafe {
                            ::aelys_native::alloc_string_from_context(_context, &error)
                        };
                        if let Ok(message) = message {
                            unsafe {
                                *out = message;
                            }
                        }
                        Err(::aelys_native::AELYS_NATIVE_RESULT_ERROR)
                    }
                }
            },
            export_type,
        ));
    }

    scalar_return_conversion(&ty_str, quote! { ret })
}

fn scalar_return_conversion(
    ty_str: &str,
    value: TokenStream2,
) -> syn::Result<(TokenStream2, ExportType)> {
    let (conversion, export_type) = match ty_str {
        "i64" => (
            quote! {
                ::aelys_native::value_int(#value)
                    .ok_or(::aelys_native::AELYS_NATIVE_INTEGER_OVERFLOW)
            },
            ExportType::Int,
        ),
        "f64" => (
            quote! { Ok(::aelys_native::value_float(#value)) },
            ExportType::Float,
        ),
        "bool" => (
            quote! { Ok(::aelys_native::value_bool(#value)) },
            ExportType::Bool,
        ),
        "()" => (
            quote! { { #value; Ok(::aelys_native::value_unit()) } },
            ExportType::Unit,
        ),
        "String" => (
            quote! {
                unsafe {
                    ::aelys_native::alloc_string_from_context(_context, &#value)
                }
            },
            ExportType::String,
        ),
        _ => {
            return unsupported_return_type_from_name(ty_str);
        }
    };

    Ok((conversion, export_type))
}

fn unsupported_return_type(ty: &Type, ty_str: &str) -> syn::Result<(TokenStream2, ExportType)> {
    Err(syn::Error::new(
        ty.span(),
        format!(
            "unsupported return type: {}. Supported: i64, f64, bool, String, (), Option<T>, Result<T, String>",
            ty_str
        ),
    ))
}

fn unsupported_return_type_from_name(ty_str: &str) -> syn::Result<(TokenStream2, ExportType)> {
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        format!(
            "unsupported return type: {}. Supported: i64, f64, bool, String, (), Option<T>, Result<T, String>",
            ty_str
        ),
    ))
}
