const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const packageJson = require("../package.json");

const root = path.resolve(__dirname, "..");
const prebuiltDir = path.join(root, "prebuilt");
const prebuiltBin = path.join(prebuiltDir, "anthmorph");
const releaseBin = path.join(root, "target", "release", "anthmorph");
const expectedVersion = packageJson.version;
const isTermux =
  process.env.TERMUX_VERSION !== undefined ||
  process.env.PREFIX === "/data/data/com.termux/files/usr";

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
  if (fs.existsSync(prebuiltBin)) {
    fs.chmodSync(prebuiltBin, 0o755);
  }
}

function buildRelease() {
  if (!hasCargo()) {
    console.error(
      [
        "[anthmorph] cargo not found; cannot build a local Linux/macOS binary.",
        "[anthmorph] Supported install paths:",
        "  1. install Rust/Cargo and rerun the install",
        "  2. run scripts/docker_build_linux.sh from the package checkout",
        "  3. use Termux on Android/aarch64 to consume the bundled prebuilt",
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

fs.mkdirSync(prebuiltDir, { recursive: true });
ensurePrebuiltPermissions();

if (isTermux && isExecutable(prebuiltBin) && binaryVersion(prebuiltBin) === expectedVersion) {
  console.log(`[anthmorph] using packaged Termux prebuilt ${expectedVersion}`);
  process.exit(0);
}

if (os.platform() === "linux" || os.platform() === "darwin") {
  console.log("[anthmorph] building local release binary for this platform");
  buildRelease();
  process.exit(0);
}

console.log("[anthmorph] unsupported platform for automatic setup; packaged files kept as-is");
