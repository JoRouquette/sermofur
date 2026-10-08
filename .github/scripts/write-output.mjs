// Writes `name=value` to the GitHub Actions step output, so that later steps of the release
// job know which version semantic-release decided to publish. Outside GitHub Actions (local
// dry run), it only prints the value.
import { appendFileSync } from "node:fs";

const [name, value] = process.argv.slice(2);
if (!name || !value || !/^[a-z]+$/.test(name) || !/^[0-9A-Za-z.+-]+$/.test(value)) {
  console.error("usage: write-output.mjs <name> <value>");
  process.exit(1);
}
if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${value}\n`);
}
console.log(`${name}=${value}`);
