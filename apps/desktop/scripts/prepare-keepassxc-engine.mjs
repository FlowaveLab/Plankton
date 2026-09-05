import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { get } from "node:https";
import { basename, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const desktopDirectory = resolve(scriptDirectory, "..");
const workspaceDirectory = resolve(desktopDirectory, "../..");
const manifest = JSON.parse(
  readFileSync(
    join(workspaceDirectory, "engines/keepassxc/manifest.json"),
    "utf8",
  ),
);
const version = manifest.version;
if (typeof version !== "string" || version.trim().length === 0) {
  throw new Error("KeePassXC manifest version is missing");
}
const target = resolveTarget();
const artifact = manifest.artifacts[target];
if (!artifact) {
  throw new Error(`KeePassXC ${version} has no pinned artifact for ${target}`);
}

const cacheDirectory = join(desktopDirectory, ".engine-cache", version);
const runtimeDirectory = join(desktopDirectory, "src-tauri/engines/keepassxc");
const archivePath = join(cacheDirectory, artifact.archive);
mkdirSync(cacheDirectory, { recursive: true });
await downloadIfMissing(
  `https://github.com/keepassxreboot/keepassxc/releases/download/${version}/${artifact.archive}`,
  archivePath,
);
verifySha256(archivePath, artifact.sha256);

rmSync(runtimeDirectory, { force: true, recursive: true });
mkdirSync(runtimeDirectory, { recursive: true });
writeFileSync(join(runtimeDirectory, ".gitkeep"), "");
const executable = prepareRuntime(archivePath, runtimeDirectory);
if (process.platform !== "win32") chmodSync(executable, 0o755);
// Keep build metadata outside the upstream signed macOS bundle.
const digestPath =
  process.platform === "darwin"
    ? join(runtimeDirectory, "keepassxc-cli.sha256")
    : `${executable}.sha256`;
writeFileSync(digestPath, `${sha256(executable)}\n`, {
  mode: 0o644,
});
if (process.platform === "darwin") {
  run("codesign", [
    "--verify",
    "--deep",
    "--strict",
    join(runtimeDirectory, "KeePassXC.app"),
  ]);
}
copyFileSync(
  join(workspaceDirectory, "engines/keepassxc/manifest.json"),
  join(runtimeDirectory, "manifest.json"),
);
await downloadIfMissing(
  `https://raw.githubusercontent.com/keepassxreboot/keepassxc/${version}/COPYING`,
  join(runtimeDirectory, "LICENSE.keepassxc.txt"),
);

function resolveTarget() {
  if (process.platform === "darwin" && process.arch === "arm64") {
    return "aarch64-apple-darwin";
  }
  if (process.platform === "darwin" && process.arch === "x64") {
    return "x86_64-apple-darwin";
  }
  if (process.platform === "win32" && process.arch === "x64") {
    return "x86_64-pc-windows-msvc";
  }
  if (process.platform === "linux" && process.arch === "x64") {
    return "x86_64-unknown-linux-gnu";
  }
  throw new Error(
    `unsupported KeePassXC target ${process.platform}/${process.arch}`,
  );
}

function prepareRuntime(archive, destination) {
  if (process.platform === "darwin") {
    const mountPoint = join(cacheDirectory, "mount");
    rmSync(mountPoint, { force: true, recursive: true });
    mkdirSync(mountPoint, { recursive: true });
    run("hdiutil", [
      "attach",
      "-nobrowse",
      "-readonly",
      "-mountpoint",
      mountPoint,
      archive,
    ]);
    try {
      const app = findNamed(mountPoint, "KeePassXC.app");
      cpSync(app, join(destination, "KeePassXC.app"), { recursive: true });
    } finally {
      run("hdiutil", ["detach", mountPoint]);
    }
    return join(destination, "KeePassXC.app/Contents/MacOS/keepassxc-cli");
  }
  if (process.platform === "win32") {
    const extracted = join(cacheDirectory, "windows-extracted");
    rmSync(extracted, { force: true, recursive: true });
    mkdirSync(extracted, { recursive: true });
    run("tar", ["-xf", archive, "-C", extracted]);
    const executable = findNamed(extracted, "keepassxc-cli.exe");
    cpSync(dirname(executable), destination, { recursive: true });
    return join(destination, basename(executable));
  }
  const executable = join(destination, "KeePassXC.AppImage");
  copyFileSync(archive, executable);
  return executable;
}

function findNamed(root, name) {
  const found = findNamedBelow(root, name);
  if (found !== undefined) return found;
  throw new Error(`${name} was not found below ${root}`);
}

function findNamedBelow(root, name) {
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.name === name) return path;
    if (entry.isDirectory()) {
      const found = findNamedBelow(path, name);
      if (found !== undefined) return found;
    }
  }
  return undefined;
}

function run(program, args) {
  const result = spawnSync(program, args, { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${program} exited with ${result.status}: ${result.stderr.trim()}`,
    );
  }
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function verifySha256(path, expected) {
  const actual = sha256(path);
  if (actual !== expected) {
    throw new Error(
      `KeePassXC archive checksum mismatch: expected ${expected}, received ${actual}`,
    );
  }
}

async function downloadIfMissing(url, destination) {
  if (existsSync(destination)) return;
  mkdirSync(dirname(destination), { recursive: true });
  const temporary = `${destination}.${process.pid}.tmp`;
  const response = await request(url);
  writeFileSync(temporary, response, { mode: 0o600 });
  copyFileSync(temporary, destination);
  rmSync(temporary);
}

function request(url) {
  return new Promise((resolvePromise, reject) => {
    get(url, { headers: { "user-agent": "plankton-build" } }, (response) => {
      if (
        response.statusCode &&
        response.statusCode >= 300 &&
        response.statusCode < 400 &&
        response.headers.location
      ) {
        response.resume();
        request(response.headers.location).then(resolvePromise, reject);
        return;
      }
      if (response.statusCode !== 200) {
        reject(
          new Error(`download ${url} failed with HTTP ${response.statusCode}`),
        );
        response.resume();
        return;
      }
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolvePromise(Buffer.concat(chunks)));
      response.on("error", reject);
    }).on("error", reject);
  });
}
