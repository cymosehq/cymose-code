import { GraphStore } from "./graph.js";
import { diffText, formatSummary, treeListing } from "./inherit.js";
import type { NodeStatus } from "./ir.js";

export type ToolSpec = {
	name: string;
	description: string;
	inputSchema: {
		type: "object";
		properties: Record<string, { type: "string"; description: string }>;
		required?: string[];
	};
};

const STATUSES: NodeStatus[] = ["active", "done", "failed", "dead-end"];

export const toolSpecs: ToolSpec[] = [
	{
		name: "cymose_tree",
		description:
			"Show the Cymose session graph: what was tried, what failed, what is focused. Call this to see context.",
		inputSchema: { type: "object", properties: {} },
	},
	{
		name: "cymose_branch",
		description:
			"Start a new Cymose node (a short session). Children inherit ancestor summaries. Omit parent_id to fork from the focused node, or to create a root if the graph is empty.",
		inputSchema: {
			type: "object",
			properties: {
				title: { type: "string", description: "What this session is trying to do" },
				parent_id: { type: "string", description: "Parent node id. Default: currently focused node." },
			},
			required: ["title"],
		},
	},
	{
		name: "cymose_focus",
		description: "Set the active Cymose node. Later inherit/summarize/promote calls use this node.",
		inputSchema: {
			type: "object",
			properties: {
				id: { type: "string", description: "Node id from cymose_tree" },
			},
			required: ["id"],
		},
	},
	{
		name: "cymose_inherit",
		description:
			"Return ancestor summaries for the focused node (or id). Read this before repeating an approach that already failed.",
		inputSchema: {
			type: "object",
			properties: {
				id: { type: "string", description: "Node id. Default: focused node." },
			},
		},
	},
	{
		name: "cymose_mark",
		description: "Set a node's status. Use dead-end or failed when an approach should not be repeated by children.",
		inputSchema: {
			type: "object",
			properties: {
				status: { type: "string", description: "active | done | failed | dead-end" },
				id: { type: "string", description: "Node id. Default: focused node." },
			},
			required: ["status"],
		},
	},
	{
		name: "cymose_summarize",
		description:
			"Write the summary children will inherit. You compose it using this harness's model — nothing is sent to another service. Outcome is your verdict.",
		inputSchema: {
			type: "object",
			properties: {
				task: { type: "string", description: "What this session set out to do" },
				outcome: { type: "string", description: "done | failed | unknown" },
				approach: { type: "string", description: "What was done, two or three sentences" },
				files_touched: { type: "string", description: "Comma-separated paths this session changed" },
				key_decisions: { type: "string", description: "Choices a later session should keep; semicolon-separated" },
				errors_encountered: { type: "string", description: "What went wrong and why; semicolon-separated" },
				id: { type: "string", description: "Node id. Default: focused node." },
			},
			required: ["task", "outcome", "approach"],
		},
	},
	{
		name: "cymose_explore",
		description:
			"Fork the focused node (or parent_id) into several sibling approaches. You invent the titles; each child inherits the same ancestors.",
		inputSchema: {
			type: "object",
			properties: {
				titles: { type: "string", description: "Two or more approach titles, separated by |" },
				parent_id: { type: "string", description: "Parent node. Default: focused node." },
			},
			required: ["titles"],
		},
	},
	{
		name: "cymose_diff",
		description: "Show two nodes' summaries side by side so you can compare approaches.",
		inputSchema: {
			type: "object",
			properties: {
				a: { type: "string", description: "First node id" },
				b: { type: "string", description: "Second node id" },
			},
			required: ["a", "b"],
		},
	},
	{
		name: "cymose_combine",
		description: "Write a synthesis onto the target node. You write the takeaway using this harness; it is stored locally.",
		inputSchema: {
			type: "object",
			properties: {
				target_id: { type: "string", description: "Node that receives the synthesis" },
				takeaway: { type: "string", description: "The combined conclusion" },
			},
			required: ["target_id", "takeaway"],
		},
	},
	{
		name: "cymose_promote",
		description: "Fold this node's summary (or an explicit conclusion) onto its parent so the parent sees the outcome.",
		inputSchema: {
			type: "object",
			properties: {
				id: { type: "string", description: "Child node. Default: focused node." },
				conclusion: { type: "string", description: "Override the stored summary" },
			},
		},
	},
	{
		name: "cymose_pick",
		description:
			"Copy summaries from other nodes onto the target, labeled by origin. Use this when an exact wording or a failed-path note must travel, not a new synthesis.",
		inputSchema: {
			type: "object",
			properties: {
				target_id: { type: "string", description: "Node that receives the copies" },
				source_ids: { type: "string", description: "Source node ids, separated by comma" },
			},
			required: ["target_id", "source_ids"],
		},
	},
	{
		name: "cymose_dump",
		description:
			"Return the graph as JSON. Ask the harness to keep that JSON in the workspace if you want it after this process ends. This plugin does not touch the disk.",
		inputSchema: { type: "object", properties: {} },
	},
	{
		name: "cymose_load",
		description:
			"Replace the in-process graph with JSON previously shown by cymose_dump (or read by the harness from the workspace).",
		inputSchema: {
			type: "object",
			properties: {
				json: { type: "string", description: "Graph JSON" },
			},
			required: ["json"],
		},
	},
];

export const MAP_INSTRUCTIONS =
	"Cymose is a session map, not a coding agent. Call cymose_tree or cymose_inherit before repeating work. Persist with cymose_dump; restore with cymose_load. The graph lives in this process.";

function asRecord(args: unknown): Record<string, unknown> {
	if (!args || typeof args !== "object" || Array.isArray(args)) return {};
	return args as Record<string, unknown>;
}

function str(args: Record<string, unknown>, key: string): string | undefined {
	const value = args[key];
	if (value === undefined || value === null) return undefined;
	if (typeof value !== "string") throw new Error(`${key} must be a string.`);
	return value;
}

function req(args: Record<string, unknown>, key: string): string {
	const value = str(args, key);
	if (!value) throw new Error(`${key} is required.`);
	return value;
}

function splitList(value: string | undefined, sep: string): string[] {
	if (!value?.trim()) return [];
	return value
		.split(sep)
		.map((item) => item.trim())
		.filter(Boolean);
}

function requireId(store: GraphStore, id?: string): string {
	const resolved = id ?? store.load().focused_id;
	if (!resolved) throw new Error("Nothing is focused. Call cymose_branch or cymose_focus.");
	return resolved;
}

function titlesFrom(args: Record<string, unknown>): string[] {
	const raw = args.titles;
	if (Array.isArray(raw)) {
		return raw.map((item) => String(item).trim()).filter(Boolean);
	}
	return splitList(str(args, "titles"), "|");
}

function idsFrom(args: Record<string, unknown>, key: string): string[] {
	const raw = args[key];
	if (Array.isArray(raw)) {
		return raw.map((item) => String(item).trim()).filter(Boolean);
	}
	return splitList(str(args, key), ",");
}

export function runTool(store: GraphStore, name: string, rawArgs: unknown = {}): string {
	const args = asRecord(rawArgs);
	switch (name) {
		case "cymose_tree": {
			const graph = store.load();
			return treeListing(graph.nodes, graph.focused_id);
		}
		case "cymose_branch": {
			const loaded = store.load();
			let parent: string | null;
			const parentId = str(args, "parent_id");
			if (parentId) parent = parentId;
			else if (loaded.focused_id) parent = loaded.focused_id;
			else parent = null;
			const node = store.branch(req(args, "title"), parent);
			return `Created and focused ${node.id}.\n\n${store.context(node.id)}`;
		}
		case "cymose_focus": {
			const node = store.focus(req(args, "id"));
			return `Focused "${node.title}" (${node.id}), status ${node.status}.`;
		}
		case "cymose_inherit": {
			const id = str(args, "id") ?? store.load().focused_id;
			if (!id) return "Nothing is focused. Call cymose_branch or cymose_focus.";
			return store.context(id);
		}
		case "cymose_mark": {
			const status = req(args, "status") as NodeStatus;
			if (!STATUSES.includes(status)) {
				throw new Error(`status must be one of ${STATUSES.join(", ")}`);
			}
			const node = store.setStatus(requireId(store, str(args, "id")), status);
			return `"${node.title}" is now ${node.status}.`;
		}
		case "cymose_summarize": {
			const id = requireId(store, str(args, "id"));
			const outcome = req(args, "outcome");
			const text = formatSummary({
				task: req(args, "task"),
				outcome,
				approach: req(args, "approach"),
				files_touched: splitList(str(args, "files_touched"), ","),
				key_decisions: splitList(str(args, "key_decisions"), ";"),
				errors_encountered: splitList(str(args, "errors_encountered"), ";"),
			});
			store.setSummary(id, text);
			if (outcome === "failed") store.setStatus(id, "failed");
			else if (outcome === "done") store.setStatus(id, "done");
			return `Stored summary on ${id}:\n\n${text}`;
		}
		case "cymose_explore": {
			const parent = requireId(store, str(args, "parent_id"));
			const nodes = store.explore(parent, titlesFrom(args));
			return ["Forked approaches (parent stays focused):", ...nodes.map((n) => `- ${n.title} ${n.id}`), "", store.focusedContext()].join(
				"\n",
			);
		}
		case "cymose_diff": {
			const left = store.get(req(args, "a"));
			const right = store.get(req(args, "b"));
			if (!left) throw new Error(`No node ${String(args.a)}.`);
			if (!right) throw new Error(`No node ${String(args.b)}.`);
			return diffText(left, right);
		}
		case "cymose_combine": {
			const node = store.setSummary(req(args, "target_id"), req(args, "takeaway").trim());
			return `Combined takeaway stored on "${node.title}" (${node.id}).`;
		}
		case "cymose_promote": {
			const parent = store.promote(requireId(store, str(args, "id")), str(args, "conclusion"));
			return `Promoted onto "${parent.title}" (${parent.id}):\n\n${parent.promoted}`;
		}
		case "cymose_pick": {
			const node = store.pick(req(args, "target_id"), idsFrom(args, "source_ids"));
			return `Copied onto "${node.title}" (${node.id}).\n\n${node.summary}`;
		}
		case "cymose_dump":
			return store.dump();
		case "cymose_load": {
			store.restore(req(args, "json"));
			const snap = store.load();
			return treeListing(snap.nodes, snap.focused_id);
		}
		default:
			throw new Error(`Unknown tool ${name}.`);
	}
}
