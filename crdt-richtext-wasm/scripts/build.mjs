// @ts-check
import { execa, $ } from "execa";
import { resolve } from "path";
import { rmdir } from "fs/promises";
import { fileURLToPath } from "url";
import path from "path";

const __filename = fileURLToPath(import.meta.url);

const __dirname = path.dirname(__filename);

// node build.mjs debug
// node build.mjs release
// node build.mjs release web
// node build.mjs release nodejs
let profile = "dev";
let profileDir = "debug";
if (process.argv[0] == "release") {
  profile = "release";
  profileDir = "release";
}
const TARGETS = ["bundler", "nodejs"];
const startTime = performance.now();
const WasmDir = resolve(__dirname, "..");
const WasmFileName = "crdt-richtext-wasm_bg.wasm";

console.log(WasmDir);
async function build() {
  await cargoBuild();
  if (process.argv[3] != null) {
    if (!TARGETS.includes(process.argv[3])) {
      throw new Error(`Invalid target [${process.argv[3]}]`);
    }

    buildTarget(process.argv[3]);
    return;
  }

  await Promise.all(
    TARGETS.map((target) => {
      return buildTarget(target);
    })
  );

  if (profile !== "dev") {
    await Promise.all(
      TARGETS.map(async (target) => {
        const cmd = `wasm-opt -O4 ./${target}/${WasmFileName} -o ./${target}/${WasmFileName}`;
        console.log(">", cmd);
        await $`wasm-opt -O4 ./${target}/${WasmFileName} -o ./${target}/${WasmFileName}`;
      })
    );
  }

  console.log(
    "✅",
    "Build complete in",
    (performance.now() - startTime) / 1000,
    "s"
  );
}

async function cargoBuild() {
  const cmd = `cargo build --target wasm32-unknown-unknown --profile ${profile}`;
  console.log(cmd);
  const status = await $({
    stdio: "inherit",
    cwd: WasmDir,
  })`cargo build --target wasm32-unknown-unknown --profile ${profile}`;
  if (status.failed) {
    console.log(
      "❌",
      "Build failed in",
      (performance.now() - startTime) / 1000,
      "s"
    );
    process.exit(1);
  }
}

async function buildTarget(target) {
  console.log("🏗️  Building target", `[${target}]`);
  const targetDirPath = resolve(WasmDir, target);
  try {
    await rmdir(targetDirPath, { recursive: true });
    console.log("Clear directory " + targetDirPath);
  } catch (e) {}

  const cmd = `wasm-bindgen --weak-refs --target ${target} --out-dir ${target} ../target/wasm32-unknown-unknown/${profileDir}/crdt_richtext_wasm.wasm`;
  console.log(">", cmd);
  await $({
    cwd: WasmDir,
    stdout: "inherit",
  })`wasm-bindgen --weak-refs --target ${target} --out-dir ${target} ../target/wasm32-unknown-unknown/${profileDir}/crdt_richtext_wasm.wasm`;
}

build();
