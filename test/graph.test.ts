import { afterEach, describe, expect, it } from "vitest";
import { GraphStore } from "../src/graph.js";
import { inheritText } from "../src/inherit.js";

describe("GraphStore", () => {
	const stores: GraphStore[] = [];
	afterEach(() => {
		stores.length = 0;
	});

	function store() {
		const g = new GraphStore();
		stores.push(g);
		return g;
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
		expect(text).toContain("do not repeat");
		expect(text).toContain("lost updates");
		expect(text).toContain("sliding window");
	});

	it("explores siblings and promotes a child onto the parent", () => {
		const g = store();
		const root = g.branch("rate limiter", null);
		const kids = g.explore(root.id, ["token bucket", "sliding window"]);
		expect(kids).toHaveLength(2);
		expect(kids.every((k) => k.parent_id === root.id)).toBe(true);
		g.setSummary(kids[1].id, "Sliding window works.");
		const parent = g.promote(kids[1].id);
		expect(parent.promoted).toBe("Sliding window works.");
		expect(g.load().focused_id).toBe(root.id);
	});

	it("shows a failed sibling on the map", () => {
		const g = store();
		const root = g.branch("limiter", null);
		const [fail, win] = g.explore(root.id, ["bucket", "window"]);
		g.setStatus(fail.id, "failed");
		g.setSummary(fail.id, "Races under load");
		g.focus(win.id);
		const text = g.context(win.id);
		expect(text).toContain("Siblings");
		expect(text).toContain("do not repeat");
		expect(text).toContain("Races under load");
	});

	it("picks a source summary onto another node", () => {
		const g = store();
		const a = g.branch("a", null);
		const b = g.branch("b", a.id);
		g.setSummary(a.id, "JWT expiry fixed");
		g.pick(b.id, [a.id]);
		expect(g.get(b.id)?.summary).toContain("JWT expiry fixed");
		expect(g.get(b.id)?.summary).toContain("From \"a\"");
	});

	it("round-trips dump and restore", () => {
		const g = store();
		g.branch("root", null);
		const raw = g.dump();
		const other = new GraphStore();
		other.restore(raw);
		expect(other.load().nodes).toHaveLength(1);
		expect(other.load().nodes[0].title).toBe("root");
	});
});
