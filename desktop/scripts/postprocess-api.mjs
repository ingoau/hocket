// typeshare emits `export enum Foo { Bar = "bar" }` for unit enums. TypeScript
// won't accept the literal "bar" (what the JSON actually carries) for that
// type, so rewrite each one to a string-literal union. Idempotent.
import { readFileSync, writeFileSync } from "node:fs";

export function postprocessApi(path) {
  const src = readFileSync(path, "utf8");
  const out = src.replace(/export enum (\w+) \{([\s\S]*?)\n\}/g, (_m, name, body) => {
    const members = [...body.matchAll(/^\s*(\w+)\s*=\s*"([^"]*)",?\s*$/gm)].map((m) => m[2]);
    if (!members.length) return _m;
    return `export type ${name} =\n${members.map((v) => `\t| "${v}"`).join("\n")};\n/** All values of {@link ${name}}, in declaration order. */\nexport const ${name}Values = [${members.map((v) => `"${v}"`).join(", ")}] as const;`;
  });
  if (out !== src) writeFileSync(path, out);
}

if (process.argv[1] && process.argv[1].endsWith("postprocess-api.mjs")) {
  postprocessApi(process.argv[2]);
}
