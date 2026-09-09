import { cp, mkdir, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
const root = fileURLToPath(new URL("..", import.meta.url));
const output = join(root, "build/site");
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
for (const path of [
  "index.html",
  "404.html",
  "style.css",
  "site.js",
  "robots.txt",
  "sitemap.xml",
  "assets",
]) {
  await cp(join(root, "site", path), join(output, path), { recursive: true });
}
console.log("Built static website in build/site");
