import { readFile, writeFile } from "node:fs/promises";
import process from "node:process";

const files = {
  package: "package.json",
  lock: "package-lock.json",
  cargo: "src-tauri/Cargo.toml",
  cargoLock: "src-tauri/Cargo.lock",
  tauri: "src-tauri/tauri.conf.json",
};

const semverPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

async function readVersions() {
  const packageJson = JSON.parse(await readFile(files.package, "utf8"));
  const packageLock = JSON.parse(await readFile(files.lock, "utf8"));
  const cargoToml = await readFile(files.cargo, "utf8");
  const cargoLock = await readFile(files.cargoLock, "utf8");
  const tauriConfig = JSON.parse(await readFile(files.tauri, "utf8"));
  const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const cargoLockVersion = cargoLock.match(/\[\[package\]\]\r?\nname = "node-watcher"\r?\nversion = "([^"]+)"/)?.[1];

  if (!cargoVersion) throw new Error(`${files.cargo}のpackage.versionを読み取れません。`);
  if (!cargoLockVersion) throw new Error(`${files.cargoLock}のnode-watcher版番号を読み取れません。`);

  return {
    packageJson,
    packageLock,
    cargoToml,
    cargoLock,
    tauriConfig,
    versions: {
      [files.package]: packageJson.version,
      [files.lock]: packageLock.version,
      [`${files.lock} packages[""]`]: packageLock.packages?.[""]?.version,
      [files.cargo]: cargoVersion,
      [files.cargoLock]: cargoLockVersion,
      [files.tauri]: tauriConfig.version,
    },
  };
}

function assertValidVersion(version) {
  if (!semverPattern.test(version)) {
    throw new Error(`版番号「${version}」はSemantic Versioning形式ではありません（例: 1.2.3）。`);
  }
}

async function check() {
  const { versions } = await readVersions();
  const expected = versions[files.package];
  assertValidVersion(expected);
  const mismatches = Object.entries(versions).filter(([, version]) => version !== expected);

  if (mismatches.length > 0) {
    const details = Object.entries(versions).map(([file, version]) => `  ${file}: ${version ?? "未設定"}`).join("\n");
    throw new Error(`版番号が一致していません。\n${details}\n\nnpm run version:set -- <version> で揃えてください。`);
  }

  console.log(`Version OK: ${expected}`);
}

async function setVersion(version) {
  assertValidVersion(version);
  const { packageJson, packageLock, cargoToml, cargoLock, tauriConfig } = await readVersions();

  packageJson.version = version;
  packageLock.version = version;
  packageLock.packages[""].version = version;
  tauriConfig.version = version;
  const updatedCargoToml = cargoToml.replace(
    /^(\[package\][\s\S]*?^version\s*=\s*)"[^"]+"/m,
    `$1"${version}"`,
  );
  const updatedCargoLock = cargoLock.replace(
    /(\[\[package\]\]\r?\nname = "node-watcher"\r?\nversion = )"[^"]+"/,
    `$1"${version}"`,
  );

  await Promise.all([
    writeFile(files.package, `${JSON.stringify(packageJson, null, 2)}\n`, "utf8"),
    writeFile(files.lock, `${JSON.stringify(packageLock, null, 2)}\n`, "utf8"),
    writeFile(files.cargo, updatedCargoToml, "utf8"),
    writeFile(files.cargoLock, updatedCargoLock, "utf8"),
    writeFile(files.tauri, `${JSON.stringify(tauriConfig, null, 2)}\n`, "utf8"),
  ]);

  console.log(`Version updated: ${version}`);
  await check();
}

const [command = "check", version] = process.argv.slice(2);

try {
  if (command === "check") await check();
  else if (command === "set" && version) await setVersion(version);
  else throw new Error("使い方: node scripts/version.mjs check | set <version>");
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
