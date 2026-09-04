import { describe, expect, it } from "vitest";
import { GraphStore, parseGraph } from "../src/graph.js";
import { inheritText } from "../src/inherit.js";
import { runTool } from "../src/tools.js";

describe("graph kinds", () => {
	it("defaults dumps without kind to session", () => {
		const g = parseGraph({
			version: 1,
			focused_id: null,
			nodes: [],
		});
		expect(g.kind).toBe("session");
	});

	it("narrates todos as blocked work, not coding sessions", () => {
		const g = new GraphStore();
		g.setKind("todo");
		const root = g.branch("ship mcp", null);
		g.setStatus(root.id, "failed");
		g.setSummary(root.id, "Blocked on catalog pin");
		const child = g.branch("write README", root.id);
		const text = inheritText(child, g.ancestors(child.id), g.siblings(child.id), "todo");
		expect(text).toContain("todo");
		expect(text).toContain("[blocked]");
		expect(text).toContain("Blocked on catalog pin");
		expect(g.context(child.id)).toContain("Parent todos");
	});

	it("narrates answer forks as rival claims", () => {
		const g = new GraphStore();
		runTool(g, "cymose_kind", { kind: "answer" });
		runTool(g, "cymose_branch", { title: "the cache is safe" });
		const parent = g.load().focused_id as string;
		runTool(g, "cymose_explore", { titles: "TTL | explicit invalidate" });
		expect(g.kind()).toBe("answer");
		expect(runTool(g, "cymose_tree")).toContain("Graph kind: answer");
		expect(parent).toBe(g.load().focused_id);
		const child = g.load().nodes.find((n) => n.parent_id === parent);
		expect(child).toBeTruthy();
		expect(g.context(child!.id)).toContain("Rival claims");
	});

	it("round-trips kind through dump", () => {
		const g = new GraphStore();
		g.setKind("steps");
		g.branch("compile", null);
		const other = new GraphStore();
		other.restore(g.dump());
		expect(other.kind()).toBe("steps");
	});
});
