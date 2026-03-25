import { docsManifest } from "../site/assets/docs-catalog.mjs";

process.stdout.write(`${JSON.stringify(docsManifest, null, 2)}
`);
