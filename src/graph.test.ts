import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { GraphStore } from "./graph.js";
import { inheritText } from "./inherit.js";

describe("GraphStore", () => {
	const dirs: string[] = [];
	afterEach(() => {
		for (const d of dirs) rmSync(d, { recursive: true, force: true });
		dirs.length = 0;
	});

	function store() {
		const dir = mkdtempSync(join(tmpdir(), "cymose-dsh-"));
		dirs.push(dir);
		return new GraphStore(join(dir, "graph.json"));
	}

	it("creates a root, then a child that inherits the parent id", () => {
		const g = store();
		const root = g.branch("auth bug", null);
		const child = g.branch("rate limiter", root.id);
		expect(child.parent_id).toBe(root.id);
		expect(g.load().focused_id).toBe(child.id);
		expect(g.ancestors(child.id).map((n) => n.title)).toEqual(["auth bug"]);
	});

	it("keeps a failed ancestor in inherit text", () => {
		const g = store();
		const root = g.branch("token bucket", null);
		g.setStatus(root.id, "failed");
		g.setSummary(root.id, "Race under contention.\nErrors: lost updates on the counter");
		const child = g.branch("sliding window", root.id);
		const text = inheritText(child, g.ancestors(child.id));
		expect(text).toContain("do not repeat this approach");
		expect(text).toContain("lost updates");
		expect(text).toContain("sliding window");
	});
});
