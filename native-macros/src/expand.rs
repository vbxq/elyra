// codegen for aelys_module attribute

use crate::args::ModuleArgs;
use crate::export::generate_export_wrapper;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Item, ItemFn, ItemMod, spanned::Spanned};

pub fn expand_module(args: ModuleArgs, mut input: ItemMod) -> syn::Result<TokenStream2> {
    let module_name = &args.name;
    let module_version = args.version.as_deref().unwrap_or("0.0.0");
    let module_prefix = module_name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let descriptor_symbol = args
        .symbol
        .unwrap_or_else(|| format!("aelys_module_descriptor_{module_prefix}"));
    let descriptor_ident = format_ident!("aelys_module_descriptor_{module_prefix}");
    let module_name_ident = format_ident!("__AELYS_MODULE_NAME_{module_prefix}");
    let module_version_ident = format_ident!("__AELYS_MODULE_VERSION_{module_prefix}");
    let exports_ident = format_ident!("__AELYS_EXPORTS_{module_prefix}");
    let init_ident = format_ident!("__aelys_module_init_{module_prefix}");
    let hash_init_ident = format_ident!("AELYS_INIT_EXPORTS_HASH_{module_prefix}");

    let (brace, content) = input.content.take().ok_or_else(|| {
        syn::Error::new(
            input.span(),
            "#[aelys_module] requires an inline module (use `mod name { ... }`)",
        )
    })?;
    let module_ident = input.ident.clone();

    let mut exports = Vec::new();
    let mut new_content = Vec::new();
    let mut wrapper_functions = Vec::new();

    for item in content {
        if let Item::Fn(func) = &item
            && has_aelys_export_attr(func)
        {
            let (wrapper, export_info) =
                generate_export_wrapper(func, &module_prefix, &module_ident)?;
            wrapper_functions.push(wrapper);
            exports.push(export_info);

            let mut clean_func = func.clone();
            clean_func
                .attrs
                .retain(|attr| !attr.path().is_ident("aelys_export"));
            new_content.push(Item::Fn(clean_func));
            continue;
        }
        new_content.push(item);
    }

    if exports.is_empty() {
        return Err(syn::Error::new(
            input.span(),
            "module has no #[aelys_export] functions",
        ));
    }

    let export_count = exports.len();
    let export_count_u32 = u32::try_from(export_count)
        .map_err(|_| syn::Error::new(input.span(), "too many native exports"))?;
    let mut export_statics = Vec::new();
    let mut export_refs = Vec::new();

    for (i, export) in exports.iter().enumerate() {
        let static_name = format_ident!("__AELYS_EXPORT_{module_prefix}_{i}");
        let name_static = format_ident!("__AELYS_EXPORT_NAME_{module_prefix}_{i}");
        let export_name = &export.name;
        let export_name_bytes = format!("{}\0", export_name);
        let arity = export.arity;
        let wrapper_name = &export.wrapper_name;

        export_statics.push(quote! {
            static #name_static: &[u8] = #export_name_bytes.as_bytes();

            static #static_name: ::aelys_native::AelysExport = ::aelys_native::AelysExport {
                name: #name_static.as_ptr() as *const ::core::ffi::c_char,
                kind: ::aelys_native::AelysExportKind::Function,
                arity: #arity,
                _padding: [0; 2],
                value: #wrapper_name as *const ::core::ffi::c_void,
            };
        });

        export_refs.push(quote! { #static_name });
    }

    let module_name_bytes = format!("{}\0", module_name);
    let module_version_bytes = format!("{}\0", module_version);

    input.content = Some((brace, new_content));

    Ok(quote! {
        #input

        #(#wrapper_functions)*

        static #module_name_ident: &[u8] = #module_name_bytes.as_bytes();
        static #module_version_ident: &[u8] = #module_version_bytes.as_bytes();

        #(#export_statics)*

        static #exports_ident: [::aelys_native::AelysExport; #export_count] = [
            #(#export_refs),*
        ];

        // Both the Rust identifier and linker symbol are module-specific, so
        // several generated modules can coexist in one binary. Dynamic
        // loading accepts this symbol as a fallback to the legacy fixed name.
        #[unsafe(export_name = #descriptor_symbol)]
        pub static mut #descriptor_ident: ::aelys_native::AelysModuleDescriptor = unsafe {
            ::aelys_native::AelysModuleDescriptor::from_raw_parts(
                ::aelys_native::AELYS_ABI_VERSION,
                ::core::mem::size_of::<::aelys_native::AelysModuleDescriptor>() as u32,
                #module_name_ident.as_ptr() as *const ::core::ffi::c_char,
                #module_version_ident.as_ptr() as *const ::core::ffi::c_char,
                ::core::ptr::null(),
                ::core::ptr::null(),
                0,
                0,
                #export_count_u32,
                #exports_ident.as_ptr(),
                0,
                ::core::ptr::null(),
                Some(#init_ident),
            )
        };

        extern "C" fn #init_ident(api: *const ::aelys_native::AelysVmApi) -> i32 {
            if api.is_null() {
                return 1;
            }
            let api = unsafe { &*api };
            ::aelys_native::store_vm_api(api);
            0
        }

        ::aelys_native::aelys_init_exports_hash!(#descriptor_ident, #hash_init_ident);
    })
}

fn has_aelys_export_attr(func: &ItemFn) -> bool {
    func.attrs
        .iter()
        .any(|attr| attr.path().is_ident("aelys_export"))
}
