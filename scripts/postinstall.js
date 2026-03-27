const { spawnSync } = require("node:child_process");
const path = require("node:path");

const root = path.resolve(__dirname, "..");

function hasCargo() {
  const probe = spawnSync("cargo", ["--version"], { stdio: "ignore" });
  return probe.status === 0;
}

if (!hasCargo()) {
  console.log("[anthmorph] cargo not found; skipping Rust build");
  process.exit(0);
}

const build = spawnSync("cargo", ["build", "--release", "--quiet"], {
  cwd: root,
  stdio: "inherit",
});

if (build.status !== 0) {
  process.exit(build.status || 1);
}
