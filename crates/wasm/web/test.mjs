// Runs the built module in Node: `node crates/wasm/web/test.mjs [path/to/mathcraft_wasm.wasm]`.
import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import { load } from "./mathcraft.js";

const path = process.argv[2] ?? new URL("./mathcraft_wasm.wasm", import.meta.url);
const mc = await load(readFileSync(path));

assert.equal(mc.run("integrate(x**2, x)").value, "x**3/3");
assert.equal(mc.run("integrate(x**2, x)").latex, "\\frac{x^{3}}{3}");
mc.run("a = 7");
assert.equal(mc.run("a * 6").value, "42");
assert.equal(mc.run("print('hi')").stdout, "hi\n");
assert.match(mc.run("1/0").error, /division by zero/);
assert.equal(mc.run("factor(x**4 - 1)").value, "(x - 1)*(x + 1)*(x**2 + 1)");
assert.equal(mc.run("solve(x**2 - 2, x)").value, "[-sqrt(2), sqrt(2)]");
assert.equal(mc.run("(1 + I)**10").value, "32*I");
// deep recursion needs the big stack
const deep = mc.run("factor(x**60 - 1)");
assert.equal(deep.error, null);
assert.match(deep.value, /x\*\*4 - x\*\*2 \+ 1/);
// a runaway loop stops at the time limit, using the host's clock
const started = performance.now();
assert.ok(mc.run("for i in range(10**9):\n    pass", 300).error);
assert.ok(performance.now() - started < 5000);
mc.reset();
assert.equal(mc.run("a * 6").value, "6*a");
console.log("wasm module: all checks passed");
