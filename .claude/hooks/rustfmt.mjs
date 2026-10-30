// PostToolUse: после правки .rs форматирует только этот файл (CI падает на cargo fmt --check).
import { spawnSync } from "node:child_process";

let raw = "";
process.stdin.on("data", (c) => (raw += c));
process.stdin.on("end", () => {
  let path = "";
  try {
    path = JSON.parse(raw).tool_input?.file_path ?? "";
  } catch {}
  if (!path.endsWith(".rs")) return;
  spawnSync("rustfmt", ["--edition", "2021", path], { stdio: "ignore" });
});
