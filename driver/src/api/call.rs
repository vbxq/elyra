use aelys_common::Result;
use aelys_common::error::{AelysError, RuntimeErrorKind};
use aelys_runtime::{self as runtime, VM, Value};

pub fn call_function(vm: &mut VM, name: &str, args: &[Value]) -> Result<Value> {
    vm.call_function_by_name(name, args)
        .map_err(AelysError::Runtime)
}

// get a cached callable for repeated calls (avoids name lookup overhead)
pub fn get_function(vm: &VM, name: &str) -> Result<CallableFunction> {
    let func_value = vm.get_function_value(name).ok_or_else(|| {
        AelysError::Runtime(
            vm.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                "function '{}' not found",
                name
            ))),
        )
    })?;

    let func_ptr = func_value.as_ptr().ok_or_else(|| {
        AelysError::Runtime(
            vm.runtime_error(RuntimeErrorKind::NotCallable("not a function".to_string())),
        )
    })?;

    let func_ref = runtime::GcRef::new(func_ptr);

    let obj = vm.heap().get(func_ref).ok_or_else(|| {
        AelysError::Runtime(vm.runtime_error(RuntimeErrorKind::NotCallable(
            "invalid reference".to_string(),
        )))
    })?;

    match &obj.kind {
        runtime::ObjectKind::Function(func) => {
            let root = vm.pin_host_ref(func_ref);
            Ok(CallableFunction {
                kind: CachedFuncKind::Function {
                    func_ref,
                    _root: root,
                    arity: func.arity(),
                },
                owner_id: vm.id(),
            })
        }
        runtime::ObjectKind::Native(native) => Ok(CallableFunction {
            kind: CachedFuncKind::Native {
                native: native.clone(),
            },
            owner_id: vm.id(),
        }),
        runtime::ObjectKind::Closure(closure) => Ok(CallableFunction {
            kind: CachedFuncKind::Closure {
                closure_ref: func_ref,
                _root: vm.pin_host_ref(func_ref),
                arity: closure.arity,
            },
            owner_id: vm.id(),
        }),
        _ => Err(AelysError::Runtime(vm.runtime_error(
            RuntimeErrorKind::NotCallable("not callable".to_string()),
        ))),
    }
}

// pre-extracted metadata for fast calls (no hashmap lookup per call). The
// object itself is deliberately retained by a host root and looked up by
// handle at call time: raw pointers into a movable heap object are not a safe
// cache key after another allocation or collection.
#[derive(Clone, Debug)]
enum CachedFuncKind {
    Function {
        func_ref: runtime::GcRef,
        _root: runtime::HostRoot,
        arity: u16,
    },
    Native {
        native: runtime::NativeFunction,
    },
    Closure {
        closure_ref: runtime::GcRef,
        _root: runtime::HostRoot,
        arity: u16,
    },
}

#[derive(Clone, Debug)]
pub struct CallableFunction {
    kind: CachedFuncKind,
    owner_id: u64,
}

impl CallableFunction {
    pub fn arity(&self) -> u16 {
        match &self.kind {
            CachedFuncKind::Function { arity, .. } | CachedFuncKind::Closure { arity, .. } => {
                *arity
            }
            CachedFuncKind::Native { native } => native.arity,
        }
    }

    pub fn is_native(&self) -> bool {
        matches!(self.kind, CachedFuncKind::Native { .. })
    }
    pub fn is_closure(&self) -> bool {
        matches!(self.kind, CachedFuncKind::Closure { .. })
    }

    pub fn call(&self, vm: &mut VM, args: &[Value]) -> Result<Value> {
        if vm.id() != self.owner_id {
            return Err(AelysError::Runtime(
                vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle),
            ));
        }

        match &self.kind {
            CachedFuncKind::Function { func_ref, .. } => vm.call_cached_function(*func_ref, args),
            CachedFuncKind::Native { native } => vm.call_cached_native(native, args),
            CachedFuncKind::Closure { closure_ref, .. } => {
                vm.call_cached_closure(*closure_ref, args)
            }
        }
        .map_err(AelysError::Runtime)
    }
}
