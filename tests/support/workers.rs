//! Runs closures on dedicated workers that share this test module and its shared memory.

use js_sys::{Array, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(inline_js = r#"
export function glue_url() {
    // Snippets live two directories below the runner's generated glue module.
    return new URL('../../wasm-bindgen-test.js', import.meta.url).href;
}

export function spawn_worker(glue, module, memory, task, timeout_ms) {
    const source = `
        import init, { run_test_worker_task } from ${JSON.stringify(glue)};
        self.onmessage = async (event) => {
            const [module, memory, task] = event.data;
            try {
                const wasm = await init({ module_or_path: module, memory });
                await run_test_worker_task(task);
                wasm.__wbindgen_thread_destroy?.();
                self.postMessage({ task, ok: true });
            } catch (error) {
                self.postMessage({ task, ok: false, error: String(error?.stack ?? error) });
            }
        };`;
    const url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    const worker = new Worker(url, { type: 'module' });
    return new Promise((resolve, reject) => {
        const finish = (settle) => {
            clearTimeout(timer);
            worker.terminate();
            URL.revokeObjectURL(url);
            settle();
        };
        const timer = setTimeout(
            () => finish(() => reject(new Error(`worker task ${task} timed out after ${timeout_ms} ms`))),
            timeout_ms,
        );
        worker.onmessage = (event) => {
            if (event.data?.task !== task) return;
            finish(() => event.data.ok ? resolve() : reject(new Error(event.data.error)));
        };
        worker.onerror = (event) => finish(() => reject(new Error(`worker task ${task}: ${event.message}`)));
        worker.postMessage([module, memory, task]);
    });
}
"#)]
extern "C" {
    fn glue_url() -> String;
    fn spawn_worker(
        glue: &str,
        module: &JsValue,
        memory: &JsValue,
        task: usize,
        timeout_ms: u32,
    ) -> Promise;
}

/// Work for one worker, returning `undefined` or a promise the worker awaits before reporting.
pub type Task = Box<dyn FnOnce() -> JsValue + Send>;

pub fn task(work: impl FnOnce() + Send + 'static) -> Task {
    Box::new(move || {
        work();
        JsValue::UNDEFINED
    })
}

/// Worker entry point: runs the boxed task whose address the spawner sent.
#[wasm_bindgen]
pub fn run_test_worker_task(task: usize) -> JsValue {
    // SAFETY: `task` came from `Box::into_raw` in `spawn` and is consumed exactly once, here.
    let task = unsafe { Box::from_raw(task as *mut Task) };
    task()
}

/// How long one worker task may run before its test fails.
const TIMEOUT_MS: u32 = 120_000;

/// Starts every task on its own worker, resolving when all finish or failing on the first error or timeout.
pub fn spawn(tasks: Vec<Task>) -> JsFuture {
    let glue = glue_url();
    let (module, memory) = (wasm_bindgen::module(), wasm_bindgen::memory());
    let promises = Array::new();
    for task in tasks {
        // The address travels to the worker as a number and comes back through `run_test_worker_task`.
        let task = Box::into_raw(Box::new(task)) as usize;
        promises.push(&spawn_worker(&glue, &module, &memory, task, TIMEOUT_MS));
    }
    JsFuture::from(Promise::all(&promises))
}
