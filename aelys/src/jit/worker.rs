use super::frame::{FrameCompiled, FrameShape, compile_frame, frame_isa, loop_entries};
use aelys_bytecode::Function;
use aelys_runtime::{
    FrameCode, FrameJobDone, FrameJobSink, FrameRequest, FrameSubmission, FrameTarget,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

const MAX_QUEUED: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Shape {
    Call,
    Loops,
    Loop(u32),
}

type CodeKey = (u64, Arc<[u32]>, Shape, bool);

enum Lookup {
    Code(Arc<FrameCompiled>),
    Refused,
    Unknown,
}

#[derive(Default)]
struct FrameCache {
    codes: HashMap<CodeKey, Arc<FrameCompiled>>,
    refused: HashSet<CodeKey>,
}

pub(crate) struct FrameCompiler {
    quick_isa: OwnedTargetIsa,
    optimized_isa: OwnedTargetIsa,
    cache: Mutex<FrameCache>,
    fault: Option<String>,
}

impl FrameCompiler {
    fn new(fault: Option<String>) -> Result<Self, String> {
        Ok(Self {
            quick_isa: frame_isa(true)?,
            optimized_isa: frame_isa(false)?,
            cache: Mutex::new(FrameCache::default()),
            fault,
        })
    }

    fn quick_len(&self) -> usize {
        self.cache
            .lock()
            .map(|cache| cache.codes.keys().filter(|key| !key.3).count())
            .unwrap_or(0)
    }

    fn lookup_key(&self, key: &CodeKey) -> Lookup {
        let Ok(cache) = self.cache.lock() else {
            return Lookup::Refused;
        };
        if let Some(code) = cache.codes.get(key) {
            return Lookup::Code(Arc::clone(code));
        }
        if cache.refused.contains(key) {
            return Lookup::Refused;
        }
        Lookup::Unknown
    }

    fn lookup(
        &self,
        module: u64,
        path: &Arc<[u32]>,
        target: FrameTarget,
        optimized: bool,
    ) -> Lookup {
        match target {
            FrameTarget::Call => {
                self.lookup_key(&(module, Arc::clone(path), Shape::Call, optimized))
            }
            FrameTarget::Loop(ip) => {
                match self.lookup_key(&(module, Arc::clone(path), Shape::Loops, optimized)) {
                    Lookup::Code(code) if code.info().osr_entries.contains(&ip) => {
                        Lookup::Code(code)
                    }
                    Lookup::Unknown => Lookup::Unknown,
                    Lookup::Code(_) | Lookup::Refused => {
                        self.lookup_key(&(module, Arc::clone(path), Shape::Loop(ip), optimized))
                    }
                }
            }
        }
    }

    fn record(&self, key: CodeKey, code: Option<FrameCompiled>) -> Lookup {
        let Ok(mut cache) = self.cache.lock() else {
            return Lookup::Refused;
        };
        if let Some(existing) = cache.codes.get(&key) {
            return Lookup::Code(Arc::clone(existing));
        }
        match code {
            Some(code) => {
                let code = Arc::new(code);
                cache.codes.insert(key, Arc::clone(&code));
                Lookup::Code(code)
            }
            None => {
                cache.refused.insert(key);
                Lookup::Refused
            }
        }
    }

    // a panic inside cranelift or the lowering marks the function uncompilable instead of unwinding into the vm
    fn build(
        &self,
        function: &Function,
        shape: FrameShape<'_>,
        optimized: bool,
    ) -> Option<FrameCompiled> {
        let isa = if optimized {
            &self.optimized_isa
        } else {
            &self.quick_isa
        };
        catch_unwind(AssertUnwindSafe(|| {
            if self.fault.is_some() && self.fault.as_deref() == function.name.as_deref() {
                panic!("injected frame compiler fault");
            }
            compile_frame(isa, function, shape, !optimized).ok()
        }))
        .ok()
        .flatten()
    }

    fn compile(
        &self,
        module: u64,
        path: &Arc<[u32]>,
        function: &Function,
        target: FrameTarget,
        optimized: bool,
    ) -> Option<Arc<FrameCompiled>> {
        match self.lookup(module, path, target, optimized) {
            Lookup::Code(code) => return Some(code),
            Lookup::Refused => return None,
            Lookup::Unknown => {}
        }
        match target {
            FrameTarget::Call => {
                let code = self.build(function, FrameShape::Call, optimized);
                match self.record((module, Arc::clone(path), Shape::Call, optimized), code) {
                    Lookup::Code(code) => Some(code),
                    Lookup::Refused | Lookup::Unknown => None,
                }
            }
            FrameTarget::Loop(ip) => {
                let loops_key = (module, Arc::clone(path), Shape::Loops, optimized);
                if let Lookup::Unknown = self.lookup_key(&loops_key) {
                    let mut entries = loop_entries(function);
                    if !entries.contains(&ip) {
                        entries.push(ip);
                        entries.sort_unstable();
                    }
                    let code = self.build(function, FrameShape::Loops(&entries), optimized);
                    if let Lookup::Code(code) = self.record(loops_key, code)
                        && code.info().osr_entries.contains(&ip)
                    {
                        return Some(code);
                    }
                }
                let single_key = (module, Arc::clone(path), Shape::Loop(ip), optimized);
                match self.lookup_key(&single_key) {
                    Lookup::Code(code) => return Some(code),
                    Lookup::Refused => return None,
                    Lookup::Unknown => {}
                }
                let code = self.build(function, FrameShape::Loops(&[ip]), optimized);
                match self.record(single_key, code) {
                    Lookup::Code(code) => Some(code),
                    Lookup::Refused | Lookup::Unknown => None,
                }
            }
        }
    }
}

// the compile reads only the immutable bytecode of the function, never vm state, so nothing it assumed can change before install
fn snapshot(function: &Function) -> Function {
    let mut copy = Function::new(function.name.clone(), function.arity);
    copy.num_registers = function.num_registers;
    copy.bytecode = function.bytecode.clone();
    copy.constants = function.constants.clone();
    copy.global_layout = Arc::clone(&function.global_layout);
    copy.global_layout_hash = function.global_layout_hash;
    copy.struct_schemas = function.struct_schemas.clone();
    copy.enum_schemas = function.enum_schemas.clone();
    copy.schema_ids = function.schema_ids.clone();
    copy.enum_ids = function.enum_ids.clone();
    copy.jit_unsupported_struct = function.jit_unsupported_struct;
    copy.nested_functions = function
        .nested_functions
        .iter()
        .map(|nested| {
            let mut shell = Function::new(None, nested.arity);
            shell.upvalue_descriptors = nested.upvalue_descriptors.clone();
            shell
        })
        .collect();
    copy
}

struct Job {
    module: u64,
    path: Arc<[u32]>,
    optimized: bool,
    loops: bool,
    function: Function,
    waiters: Vec<(Arc<FrameJobSink>, u64, FrameTarget)>,
}

#[derive(Default)]
struct Queue {
    quick: VecDeque<Job>,
    optimized: VecDeque<Job>,
    shutdown: bool,
}

struct Shared {
    compiler: Arc<FrameCompiler>,
    queue: Mutex<Queue>,
    wake: Condvar,
}

pub(crate) struct FrameJit {
    compiler: Arc<FrameCompiler>,
    shared: Option<Arc<Shared>>,
    worker: Option<JoinHandle<()>>,
}

impl FrameJit {
    pub(crate) fn new(asynchronous: bool, fault: Option<String>) -> Result<Self, String> {
        let compiler = Arc::new(FrameCompiler::new(fault)?);
        if !asynchronous {
            return Ok(Self {
                compiler,
                shared: None,
                worker: None,
            });
        }
        let shared = Arc::new(Shared {
            compiler: Arc::clone(&compiler),
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("aelys-jit".to_string())
            .spawn(move || run_worker(&worker_shared))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            compiler,
            shared: Some(shared),
            worker: Some(worker),
        })
    }

    pub(crate) fn quick_len(&self) -> usize {
        self.compiler.quick_len()
    }

    pub(crate) fn submit(&self, request: FrameRequest<'_>) -> FrameSubmission {
        let module = request.key.module();
        let path = request.key.shared_path();
        let ready = |code: Arc<FrameCompiled>| FrameSubmission::Ready(code as Arc<dyn FrameCode>);
        match self
            .compiler
            .lookup(module, &path, request.target, request.optimized)
        {
            Lookup::Code(code) => return ready(code),
            Lookup::Refused => return FrameSubmission::Refused,
            Lookup::Unknown => {}
        }
        let Some(shared) = &self.shared else {
            return match self.compiler.compile(
                module,
                &path,
                request.function,
                request.target,
                request.optimized,
            ) {
                Some(code) => ready(code),
                None => FrameSubmission::Refused,
            };
        };
        let loops = matches!(request.target, FrameTarget::Loop(_));
        let waiter = (Arc::clone(request.sink), request.token, request.target);
        let Ok(mut queue) = shared.queue.lock() else {
            return FrameSubmission::Refused;
        };
        if queue.shutdown {
            return FrameSubmission::Refused;
        }
        let lane = if request.optimized {
            &mut queue.optimized
        } else {
            &mut queue.quick
        };
        if let Some(job) = lane
            .iter_mut()
            .find(|job| job.module == module && job.path == path && job.loops == loops)
        {
            job.waiters.push(waiter);
            return FrameSubmission::Queued;
        }
        if queue.quick.len() + queue.optimized.len() >= MAX_QUEUED {
            return FrameSubmission::Busy;
        }
        let job = Job {
            module,
            path,
            optimized: request.optimized,
            loops,
            function: snapshot(request.function),
            waiters: vec![waiter],
        };
        if request.optimized {
            queue.optimized.push_back(job);
        } else {
            queue.quick.push_back(job);
        }
        drop(queue);
        shared.wake.notify_one();
        FrameSubmission::Queued
    }
}

impl Drop for FrameJit {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            if let Ok(mut queue) = shared.queue.lock() {
                queue.shutdown = true;
                queue.quick.clear();
                queue.optimized.clear();
            }
            shared.wake.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// the main thread runs the program and must win the cpu whenever both are runnable
fn lower_priority() {
    #[cfg(target_os = "linux")]
    unsafe {
        let thread = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, thread, 10);
    }
}

fn run_worker(shared: &Shared) {
    lower_priority();
    loop {
        let job = {
            let Ok(mut queue) = shared.queue.lock() else {
                return;
            };
            loop {
                if queue.shutdown {
                    return;
                }
                if let Some(job) = queue
                    .quick
                    .pop_front()
                    .or_else(|| queue.optimized.pop_front())
                {
                    break job;
                }
                queue = match shared.wake.wait(queue) {
                    Ok(queue) => queue,
                    Err(_) => return,
                };
            }
        };
        for (sink, token, target) in job.waiters {
            let code = shared.compiler.compile(
                job.module,
                &job.path,
                &job.function,
                target,
                job.optimized,
            );
            sink.deliver(FrameJobDone {
                token,
                target,
                code: code.map(|code| code as Arc<dyn FrameCode>),
            });
        }
    }
}
