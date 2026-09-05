import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const distDirectory = resolve(process.cwd(), "dist");
const indexHtml = await readFile(resolve(distDirectory, "index.html"), "utf8");
const stylesheetPaths = Array.from(
  indexHtml.matchAll(/<link[^>]+href="([^"]+\.css)"[^>]*>/g),
  (match) => match[1],
);

if (stylesheetPaths.length === 0) {
  throw new Error("The desktop entry point does not load a stylesheet.");
}

const entryCss = (
  await Promise.all(
    stylesheetPaths.map((pathname) =>
      readFile(resolve(distDirectory, pathname.replace(/^\//, "")), "utf8"),
    ),
  )
).join("\n");

for (const selector of [
  ".desktop-workspace .approval-chat",
  ".desktop-workspace .request-review-progress",
]) {
  if (!entryCss.includes(selector)) {
    throw new Error(`The desktop entry stylesheet is missing ${selector}.`);
  }
}
