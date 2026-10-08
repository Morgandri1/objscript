#!/usr/bin/env bun
// Usage: bun scripts/bump.ts <major|minor|patch|X.Y.Z> [--tag] [--dry]
//   --tag  commit the bumped files and create tag vX.Y.Z
//   --dry  print what would change, write nothing
import { $ } from "bun";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const files = {
  cargoRoot: join(root, "Cargo.toml"),
  cargoWasm: join(root, "wasm", "Cargo.toml"),
  packageJson: join(root, "ts", "package.json"),
};

const args = Bun.argv.slice(2);
const how = args.find((a) => !a.startsWith("--"));
const dry = args.includes("--dry");
const tag = args.includes("--tag");
if (!how) {
  console.error("usage: bun scripts/bump.ts <major|minor|patch|X.Y.Z> [--tag] [--dry]");
  process.exit(1);
}

const pkg = await Bun.file(files.packageJson).json();
const current: string = pkg.version;
const next = bump(current, how);
console.log(`${current} -> ${next}${dry ? " (dry run)" : ""}`);

function bump(version: string, how: string): string {
  if (/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(how)) return how;
  const [major, minor, patch] = version.split("-")[0]!.split(".").map(Number) as [number, number, number];
  switch (how) {
    case "major": return `${major + 1}.0.0`;
    case "minor": return `${major}.${minor + 1}.0`;
    case "patch": return `${major}.${minor}.${patch + 1}`;
    default: throw new Error(`expected major, minor, patch or X.Y.Z, got "${how}"`);
  }
}

/** Replace `version = "..."` in the [package] section only, leaving dependency versions alone. */
async function setCargoVersion(path: string) {
  const lines = (await Bun.file(path).text()).split("\n");
  let section = "";
  let found = false;
  for (let i = 0; i < lines.length; i++) {
    const header = lines[i]!.match(/^\s*\[\[?([^\]]+)\]\]?\s*$/);
    if (header) {
      section = header[1]!.trim();
      continue;
    }
    if (section === "package" && /^\s*version\s*=/.test(lines[i]!)) {
      lines[i] = lines[i]!.replace(/"[^"]*"/, `"${next}"`);
      found = true;
      break;
    }
  }
  if (!found) throw new Error(`${path}: no version in [package]`);
  console.log(`  ${path.replace(root + "/", "")}`);
  if (!dry) await Bun.write(path, lines.join("\n"));
}

await setCargoVersion(files.cargoRoot);
await setCargoVersion(files.cargoWasm);

pkg.version = next;
console.log(`  ${files.packageJson.replace(root + "/", "")}`);
if (!dry) await Bun.write(files.packageJson, JSON.stringify(pkg, null, 2) + "\n");

if (dry) process.exit(0);

// Refresh workspace entries in Cargo.lock without touching third-party deps.
await $`cargo update --workspace`.cwd(root).quiet();
console.log("  Cargo.lock");

if (tag) {
  await $`git add ${files.cargoRoot} ${files.cargoWasm} ${files.packageJson} Cargo.lock`.cwd(root);
  await $`git commit -m ${`release: v${next}`}`.cwd(root);
  await $`git tag ${`v${next}`}`.cwd(root);
  console.log(`\nTagged v${next}. Push with: git push --follow-tags`);
}