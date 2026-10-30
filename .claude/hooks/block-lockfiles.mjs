// PreToolUse: запрещает ручную правку lock-файлов (их обновляют cargo/bun).
let raw = "";
process.stdin.on("data", (c) => (raw += c));
process.stdin.on("end", () => {
  let path = "";
  try {
    path = JSON.parse(raw).tool_input?.file_path ?? "";
  } catch {}
  if (/(^|[\\/])(Cargo\.lock|bun\.lock)$/.test(path)) {
    console.error(
      "Lock-файлы не правятся вручную: используйте `cargo update -p ...`/`cargo check` или `bun install`.",
    );
    process.exit(2);
  }
});
