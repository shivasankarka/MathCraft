// Runs the engine off the page's thread, so a long computation cannot freeze the page.
import { load } from "./mathcraft.js";

const ready = load(new URL("./mathcraft_wasm.wasm", import.meta.url).href);

self.onmessage = async ({ data }) => {
  const mc = await ready;
  if (data.reset) {
    mc.reset();
    self.postMessage({ id: data.id, reset: true });
    return;
  }
  self.postMessage({ id: data.id, ...mc.run(data.code, data.timeoutMs ?? 20000) });
};
