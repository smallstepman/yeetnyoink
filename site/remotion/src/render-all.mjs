import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { allDocFields } from "../../assets/docs-catalog.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const remotionRoot = resolve(here, "..");
const entryPoint = resolve(remotionRoot, "src/root.jsx");

for (const field of allDocFields) {
  const outputPath = resolve(remotionRoot, "..", field.assetPath);
  mkdirSync(dirname(outputPath), { recursive: true });
  execFileSync(
    "remotion",
    [
      "render",
      entryPoint,
      field.compositionId,
      outputPath,
      "--codec=h264",
      "--pixel-format=yuv420p",
      "--overwrite",
    ],
    {
      cwd: remotionRoot,
      stdio: "inherit",
    },
  );
}
