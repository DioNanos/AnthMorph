const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const packageJson = require("../package.json");

const root = path.resolve(__dirname, "..");
const prebuiltDir = path.join(root, "prebuilt");
const releaseBin = path.join(root, "target", "release", "anthmorph");
const defaultConfigScript = path.join(root, "scripts", "write_default_config.py");
const expectedVersion = packageJson.version;
const isTermux =
  process.env.TERMUX_VERSION !== undefined ||
  process.env.PREFIX === "/data/data/com.termux/files/usr";

function targetTriple() {
  if (isTermux && os.arch() === "arm64") {
    return "android-arm64";
  }
  if (os.platform() === "linux" && os.arch() === "x64") {
    return "linux-x64";
  }
  if (os.platform() === "darwin" && os.arch() === "arm64") {
    return "darwin-arm64";
  }
  return null;
}

const prebuiltTarget = targetTriple();
const prebuiltBin = prebuiltTarget ? path.join(prebuiltDir, prebuiltTarget, "anthmorph") : null;

function hasCargo() {
  const probe = spawnSync("cargo", ["--version"], { stdio: "ignore" });
  return probe.status === 0;
}

function isExecutable(file) {
  try {
    fs.accessSync(file, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

function binaryVersion(file) {
  try {
    const out = spawnSync(file, ["--version"], { encoding: "utf8" });
    if (out.status !== 0) return null;
    const match = out.stdout.trim().match(/(\d+\.\d+\.\d+)$/);
    return match ? match[1] : null;
  } catch {
    return null;
  }
}

function ensurePrebuiltPermissions() {
  if (prebuiltBin && fs.existsSync(prebuiltBin)) {
    fs.chmodSync(prebuiltBin, 0o755);
  }
}

function buildRelease() {
  if (!hasCargo()) {
    console.error(
      [
        "[anthmorph] cargo not found and no matching packaged binary is available.",
        "[anthmorph] Supported install paths:",
        "  1. use Linux x64, macOS arm64, or Termux Android arm64 prebuilt packages",
        "  2. install Rust/Cargo and rerun the install",
        "  3. run scripts/docker_build_linux.sh from the package checkout",
        "[anthmorph] See docs/PACKAGING.md for details.",
      ].join("\n"),
    );
    process.exit(1);
  }

  const build = spawnSync("cargo", ["build", "--release", "--quiet"], {
    cwd: root,
    stdio: "inherit",
  });

  if (build.status !== 0) {
    process.exit(build.status || 1);
  }
}

function syncReleaseToPrebuilt() {
  if (!prebuiltBin || !fs.existsSync(releaseBin)) {
    return;
  }
  fs.mkdirSync(path.dirname(prebuiltBin), { recursive: true });
  fs.copyFileSync(releaseBin, prebuiltBin);
  fs.chmodSync(prebuiltBin, 0o755);
  if (os.platform() === "darwin") {
    spawnSync("codesign", ["--force", "--sign", "-", prebuiltBin], {
      stdio: "ignore",
    });
  }
}

function bootstrapDefaultConfig() {
  const configDir = path.join(os.homedir(), ".config", "anthmorph");
  const configFile = path.join(configDir, "config.toml");
  if (fs.existsSync(configFile)) {
    return;
  }
  fs.mkdirSync(configDir, { recursive: true });
  const write = spawnSync("python3", [defaultConfigScript, configFile], {
    cwd: root,
    stdio: "inherit",
  });
  if (write.status !== 0) {
    console.warn("[anthmorph] failed to bootstrap default config");
  } else {
    console.log(`[anthmorph] wrote default config to ${configFile}`);
  }
}

fs.mkdirSync(prebuiltDir, { recursive: true });
ensurePrebuiltPermissions();

if (prebuiltBin && isExecutable(prebuiltBin) && binaryVersion(prebuiltBin) === expectedVersion) {
  console.log(`[anthmorph] using packaged ${prebuiltTarget} prebuilt ${expectedVersion}`);
  bootstrapDefaultConfig();
  process.exit(0);
}

if (os.platform() === "linux" || os.platform() === "darwin") {
  console.log("[anthmorph] building local release binary for this platform");
  buildRelease();
  syncReleaseToPrebuilt();
  bootstrapDefaultConfig();
  process.exit(0);
}

console.log("[anthmorph] unsupported platform for automatic setup; packaged files kept as-is");
