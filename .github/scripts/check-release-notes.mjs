// Runs @semantic-release/commit-analyzer and @semantic-release/release-notes-generator with their
// options from .releaserc.json on sample commits, without git, network or token. The `plan` job
// renders notes only on main when a release is due: this check catches an incompatible update of
// these two plugins or of their preset in the pull request. The semantic-release core and the
// exec and github plugins are only exercised by `plan`.
import { readFileSync } from "node:fs";
import { analyzeCommits } from "@semantic-release/commit-analyzer";
import { generateNotes } from "@semantic-release/release-notes-generator";

const config = JSON.parse(readFileSync(".releaserc.json", "utf8"));
const optionsOf = (name) => {
  const entry = config.plugins.find((plugin) => Array.isArray(plugin) && plugin[0] === name);
  if (!entry || entry[1]?.preset !== "conventionalcommits") {
    throw new Error(`${name} must be configured with the conventionalcommits preset in .releaserc.json.`);
  }
  return entry[1];
};
const analyzerOptions = optionsOf("@semantic-release/commit-analyzer");
const notesOptions = optionsOf("@semantic-release/release-notes-generator");

const sha = (n) => n.toString().padStart(40, "0");
async function check(messages, expectedType, expectedTexts) {
  const context = {
    cwd: process.cwd(),
    env: {},
    commits: messages.map((message, index) => ({ hash: sha(index + 1), message })),
    logger: { log() {}, error: console.error },
    options: { repositoryUrl: "https://github.com/JoRouquette/sermofur" },
    lastRelease: { gitTag: "v1.0.0", version: "1.0.0" },
    nextRelease: { gitTag: "v2.0.0", version: "2.0.0", type: expectedType },
  };
  const type = await analyzeCommits(analyzerOptions, context);
  const notes = await generateNotes(notesOptions, context);
  const missing = expectedTexts.filter((text) => !notes.includes(text));
  if (type !== expectedType || missing.length > 0) {
    console.error(`Expected ${expectedType}, got ${type}; missing ${missing.join(", ")}\n${notes}`);
    process.exit(1);
  }
  console.log(`Release type ${type}; notes rendered:\n${notes}`);
}

await check(
  ["fix(cli): sample fix", "feat: sample feature", "ci: sample change"],
  "minor",
  ["sample fix", "sample feature"],
);
await check(
  ["feat!: sample breaking feature", "fix: sample fix\n\nBREAKING CHANGE: sample breaking note"],
  "major",
  ["sample breaking feature", "sample breaking note"],
);
