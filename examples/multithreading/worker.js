// synchronously, using the browser, import out shim JS scripts
// import init, {child_entry_point} from "http://127.0.0.1:8000/pkg/wasm_thread.js";

// Wait for the main thread to send us the shared module/memory. Once we've got
// it, initialize it all with the `wasm_bindgen` global we imported via
// `importScripts`.
//
// After our first message all subsequent messages are an entry point to run,
// so we just do that.
self.onmessage =  event => {
  let initialised = import('http://127.0.0.1:8000/pkg/multithreading.js').then((x) => x.default(event.data[0], event.data[1]));

  self.onmessage = async event => {
    // This will queue further commands up until the module is fully initialised:
    const module  = await initialised;
    module.child_entry_point(event.data);
  };
};;;
