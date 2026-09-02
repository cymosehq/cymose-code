import { describe, expect, it } from "vitest";
import { GraphStore } from "../src/graph.js";
import { runTool } from "../src/tools.js";
import { createMcpHandler } from "../mcp/protocol.js";

describe("MCP adapter", () => {
	it("tags new nodes as mcp", () => {
		const store = new GraphStore("mcp");
		expect(store.branch("root", null).host).toBe("mcp");
	});

	it("lists tools and runs a branch through JSON-RPC", () => {
		const handle = createMcpHandler(new GraphStore("mcp"));
		const listed = handle({ jsonrpc: "2.0", id: 1, method: "tools/list" });
		expect(listed?.error).toBeUndefined();
		const tools = (listed?.result as { tools: { name: string }[] }).tools;
		expect(tools.map((t) => t.name)).toContain("cymose_tree");

		const started = handle({
			jsonrpc: "2.0",
			id: 2,
			method: "initialize",
			params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "test", version: "0" } },
		});
		expect((started?.result as { protocolVersion: string }).protocolVersion).toBe("2025-03-26");

		const branched = handle({
			jsonrpc: "2.0",
			id: 3,
			method: "tools/call",
			params: { name: "cymose_branch", arguments: { title: "auth bug" } },
		});
		const body = (branched?.result as { content: { text: string }[] }).content[0].text;
		expect(body).toContain("Created and focused");
		expect(branched?.result).not.toHaveProperty("isError");
	});

	it("returns tool failures as isError content", () => {
		const handle = createMcpHandler(new GraphStore("mcp"));
		const reply = handle({
			jsonrpc: "2.0",
			id: 4,
			method: "tools/call",
			params: { name: "cymose_focus", arguments: { id: "missing" } },
		});
		expect((reply?.result as { isError?: boolean }).isError).toBe(true);
	});

	it("shares runTool with the graph store", () => {
		const store = new GraphStore("mcp");
		runTool(store, "cymose_branch", { title: "root" });
		const tree = runTool(store, "cymose_tree");
		expect(tree).toContain("root");
	});
});
