import init, {
    test_spawn
} from "./pkg/multithreading.js";

await init();

async function run_in_worker(event) {
  await test_spawn();
}

self.onmessage = function (event) {
    run_in_worker(event);
}

self.postMessage("ready");
