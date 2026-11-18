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
    Bytes,
}

impl ExportType {
    fn is_parameter(self) -> bool {
        matches!(
            self,
            Self::Int | Self::Float | Self::Bool | Self::String | Self::Unit | Self::Bytes
        )
    }

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
            Self::Bytes => quote! { Bytes },
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
    let declared = parse_export_args(func)?;

    if func.sig.abi.is_some() {
        return generate_raw_wrapper(func, declared, &wrapper_name, &export_symbol, module_ident);
    }
    if !declared.is_empty() {
        return Err(syn::Error::new(
            declared.span(),
            "`arity`, `params` and `result` are only for raw `extern \"C\"` exports; a typed export takes them from its Rust signature",
        ));
    }

    let mut param_extractions = Vec::new();
    let mut call_args = Vec::new();
    let mut param_types = Vec::new();
    let mut arity: u16 = 0;
    let mut takes_context = false;

    for arg in func.sig.inputs.iter() {
        let FnArg::Typed(PatType { pat, ty, .. }) = arg else {
            return Err(syn::Error::new(
                arg.span(),
                "an exported function cannot take `self`",
            ));
        };
        if is_call_context(ty) {
            if takes_context {
                return Err(syn::Error::new(
                    ty.span(),
                    "an exported function takes at most one `CallContext`",
                ));
            }
            takes_context = true;
            call_args.push(quote! { __aelys_ctx });
            continue;
        }
        let param_name = match &**pat {
            Pat::Ident(ident) => &ident.ident,
            _ => return Err(syn::Error::new(pat.span(), "expected identifier pattern")),
        };

        let (extraction, param_type) = generate_extraction(param_name, ty, usize::from(arity))?;
        param_extractions.push(extraction);
        param_types.push(param_type);
        call_args.push(quote! { #param_name });
        arity = arity
            .checked_add(1)
            .ok_or_else(|| syn::Error::new(arg.span(), "too many parameters"))?;
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
            let body = move |__aelys_ctx: &mut ::aelys_native::CallContext<'_>|
                -> Result<::aelys_native::AelysValue, i32> {
                #(#param_extractions)*
                let ret = #module_ident::#fn_name(#(#call_args),*);
                #return_conversion
            };
            let result = unsafe { ::aelys_native::with_call_context(_context, body) };

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

#[derive(Default)]
struct ExportArgs {
    arity: Option<(u16, proc_macro2::Span)>,
    params: Option<(Vec<ExportType>, proc_macro2::Span)>,
    result: Option<ExportType>,
}

impl ExportArgs {
    fn is_empty(&self) -> bool {
        self.arity.is_none() && self.params.is_none() && self.result.is_none()
    }

    fn span(&self) -> proc_macro2::Span {
        self.arity
            .map(|(_, span)| span)
            .or_else(|| self.params.as_ref().map(|(_, span)| *span))
            .unwrap_or_else(proc_macro2::Span::call_site)
    }
}

fn parse_export_args(func: &ItemFn) -> syn::Result<ExportArgs> {
    let mut args = ExportArgs::default();
    for attr in &func.attrs {
        if !attr.path().is_ident("aelys_export") || matches!(attr.meta, syn::Meta::Path(_)) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("arity") {
                let value: syn::LitInt = meta.value()?.parse()?;
                args.arity = Some((value.base10_parse::<u16>()?, value.span()));
                Ok(())
            } else if meta.path.is_ident("params") {
                let list: syn::ExprArray = meta.value()?.parse()?;
                let mut params = Vec::new();
                for element in &list.elems {
                    let param = named_export_type(element)?;
                    if !param.is_parameter() {
                        return Err(syn::Error::new(
                            element.span(),
                            "sum types are valid only as function results",
                        ));
                    }
                    params.push(param);
                }
                args.params = Some((params, list.span()));
                Ok(())
            } else if meta.path.is_ident("result") {
                let expr: syn::Expr = meta.value()?.parse()?;
                let result = named_export_type(&expr)?;
                if matches!(result, ExportType::Bytes) {
                    return Err(syn::Error::new(
                        expr.span(),
                        "byte buffers are valid only as function parameters",
                    ));
                }
                args.result = Some(result);
                Ok(())
            } else {
                Err(meta.error(
                    "unknown `aelys_export` argument; expected `arity = N`, `params = [..]` or `result = T`",
                ))
            }
        })?;
    }
    Ok(args)
}

fn named_export_type(expr: &syn::Expr) -> syn::Result<ExportType> {
    let syn::Expr::Path(path) = expr else {
        return Err(syn::Error::new(expr.span(), "expected a native type name"));
    };
    let name = path
        .path
        .get_ident()
        .map(ToString::to_string)
        .unwrap_or_default();
    Ok(match name.as_str() {
        "Int" => ExportType::Int,
        "Float" => ExportType::Float,
        "Bool" => ExportType::Bool,
        "String" => ExportType::String,
        "Unit" => ExportType::Unit,
        "Bytes" => ExportType::Bytes,
        "OptionInt" => ExportType::OptionInt,
        "OptionFloat" => ExportType::OptionFloat,
        "OptionBool" => ExportType::OptionBool,
        "OptionString" => ExportType::OptionString,
        "OptionUnit" => ExportType::OptionUnit,
        "ResultIntString" => ExportType::ResultIntString,
        "ResultFloatString" => ExportType::ResultFloatString,
        "ResultBoolString" => ExportType::ResultBoolString,
        "ResultStringString" => ExportType::ResultStringString,
        "ResultUnitString" => ExportType::ResultUnitString,
        _ => {
            return Err(syn::Error::new(
                expr.span(),
                "unknown native type; expected one of Int, Float, Bool, String, Unit, Bytes, OptionInt, ..., ResultUnitString",
            ));
        }
    })
}

fn generate_raw_wrapper(
    func: &ItemFn,
    declared: ExportArgs,
    wrapper_name: &Ident,
    export_symbol: &str,
    module_ident: &Ident,
) -> syn::Result<(TokenStream2, ExportInfo)> {
    let fn_name = &func.sig.ident;
    let is_c_abi = func
        .sig
        .abi
        .as_ref()
        .and_then(|abi| abi.name.as_ref())
        .is_some_and(|name| name.value() == "C");
    if !is_c_abi || func.sig.inputs.len() != 4 {
        return Err(syn::Error::new(
            func.sig.span(),
            "a raw export must be `unsafe extern \"C\" fn(*mut NativeContext, *const AelysValue, usize, *mut AelysValue) -> i32`",
        ));
    }
    let signature = match (declared.params, declared.result) {
        (Some((params, _)), Some(result)) => Some((params, result)),
        (Some((params, _)), None) => Some((params, ExportType::Unit)),
        (None, Some(_)) => {
            return Err(syn::Error::new(
                func.sig.span(),
                "a raw export with `result` must also list its `params` (write `params = []` for none)",
            ));
        }
        (None, None) => None,
    };
    let arity = match (&signature, declared.arity) {
        (Some((params, _)), Some((arity, span))) if usize::from(arity) != params.len() => {
            return Err(syn::Error::new(
                span,
                "`arity` disagrees with the number of `params`",
            ));
        }
        (Some((params, _)), _) => u16::try_from(params.len())
            .map_err(|_| syn::Error::new(func.sig.span(), "too many parameters"))?,
        (None, Some((arity, _))) => arity,
        (None, None) => {
            return Err(syn::Error::new(
                func.sig.span(),
                "a raw export must declare its signature: `#[aelys_export(params = [Int, Bytes], result = Int)]`, or at least `#[aelys_export(arity = N)]` for an untyped export",
            ));
        }
    };
    let call = if func.sig.unsafety.is_some() {
        quote! { unsafe { #module_ident::#fn_name(context, args, arg_count, out) } }
    } else {
        quote! { #module_ident::#fn_name(context, args, arg_count, out) }
    };

    let wrapper = quote! {
        #[doc(hidden)]
        #[unsafe(export_name = #export_symbol)]
        pub unsafe extern "C" fn #wrapper_name(
            context: *mut ::aelys_native::NativeContext,
            args: *const ::aelys_native::AelysValue,
            arg_count: usize,
            out: *mut ::aelys_native::AelysValue,
        ) -> i32 {
            if out.is_null()
                || arg_count != usize::from(#arity)
                || (arg_count != 0 && args.is_null())
            {
                return ::aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
            }
            #call
        }
    };

    Ok((
        wrapper,
        ExportInfo {
            name: fn_name.to_string(),
            arity,
            wrapper_name: wrapper_name.clone(),
            signature,
        },
    ))
}

fn is_call_context(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    let Type::Path(path) = &*reference.elem else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "CallContext")
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
        "&mut[u8]" => (
            quote! {
                let #name: &mut [u8] = __aelys_ctx.borrow_bytes(unsafe { *args.add(#idx) })?;
            },
            ExportType::Bytes,
        ),
        "&[u8]" => (
            quote! {
                let #name: &[u8] = __aelys_ctx.borrow_bytes_ref(unsafe { *args.add(#idx) })?;
            },
            ExportType::Bytes,
        ),
        _ => {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "unsupported parameter type: {}. Supported: i64, f64, bool, String, &[u8], &mut [u8], &mut CallContext",
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
