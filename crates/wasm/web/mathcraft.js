// Loads the MathCraft engine (mathcraft_wasm.wasm) and runs code in it.
//
//   import { load } from "./mathcraft.js";
//   const mc = await load("mathcraft_wasm.wasm");       // or pass the bytes (ArrayBuffer) directly
//   mc.run("integrate(x**2, x)");   // { stdout, value, latex, error }
//   mc.reset();                      // forget the variables

export async function load(source) {
  const bytes = source instanceof ArrayBuffer || ArrayBuffer.isView(source) ? source : await (await fetch(source)).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, { env: { mc_now_ms: () => performance.now() } });
  const x = instance.exports;
  x.mc_init();
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  return {
    /** Run `code`; resolves nothing asynchronously, so a long computation blocks the page (use a worker for that). */
    run(code, timeoutMs = 10000) {
      const input = encoder.encode(code);
      const ptr = x.mc_alloc(input.length);
      new Uint8Array(x.memory.buffer, ptr, input.length).set(input);
      const length = x.mc_run(ptr, input.length, timeoutMs);
      x.mc_free(ptr, input.length);
      return JSON.parse(decoder.decode(new Uint8Array(x.memory.buffer, x.mc_output(), length)));
    },
    reset() {
      x.mc_reset();
    },
  };
}
