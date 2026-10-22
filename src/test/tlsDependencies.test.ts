/// <reference types="node" />
// @vitest-environment node
import { readFileSync } from "node:fs";

const lockfile = readFileSync(new URL("../../src-tauri/Cargo.lock", import.meta.url), "utf8")
  .replace(/\r\n/g, "\n");

// Keep every locked copy on the patched release line. These checks guard against
// restoring an old lockfile; TLS behavior is implemented and tested upstream.
test.each([
  ["rustls-webpki", [0, 103, 16]],
  ["webpki-roots", [1, 0, 10]],
] as const)("%s includes the eMudhra distrust update", (name, minimum) => {
  const versions = lockfile
    .split("[[package]]")
    .filter((entry) => entry.includes(`\nname = "${name}"\n`))
    .map((entry) => /^version = "([^"]+)"$/m.exec(entry)?.[1]);

  expect(versions.length).toBeGreaterThan(0);
  for (const version of versions) {
    expect(version).toMatch(/^\d+\.\d+\.\d+$/);
    const [major, minor, patch] = version!.split(".").map(Number);
    // A new release line needs a fresh security review, not an assumed pass.
    expect([major, minor]).toEqual(minimum.slice(0, 2));
    expect(patch).toBeGreaterThanOrEqual(minimum[2]);
  }
});
