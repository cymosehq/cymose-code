import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";

/** Stable device id for Cymose metering. One per machine, not per graph. */
export function deviceId(): string {
	const dir = join(homedir(), ".config", "cymose");
	const path = join(dir, "device");
	try {
		const existing = readFileSync(path, "utf8").trim();
		if (existing) return existing;
	} catch {
		// first run
	}
	const id = randomUUID();
	mkdirSync(dir, { recursive: true });
	writeFileSync(path, `${id}\n`, { encoding: "utf8", mode: 0o600 });
	return id;
}

export function defaultGraphPath(cwd = process.cwd()): string {
	return join(cwd, ".cymose", "graph.json");
}
