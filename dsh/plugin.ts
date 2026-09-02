import type { Context } from "@deepseek-ai/cordis";
import { defineTool } from "@deepseek-ai/dsh-tools";
import { GraphStore } from "../src/graph.js";
import { diffText, formatSummary, treeListing } from "../src/inherit.js";
import type { NodeStatus } from "../src/ir.js";

export const name = "cymose";

export const inject = ["tools"];

export interface Config {
	/** In-process graph name if you keep more than one. */
	namespace?: string;
}

export const Config: Config = {
	namespace: "",
};

const graphs = new Map<string, GraphStore>();

function store(config: Config): GraphStore {
	const key = config.namespace?.trim() || "default";
	const existing = graphs.get(key);
	if (existing) return existing;
	const created = new GraphStore();
	graphs.set(key, created);
	return created;
}

function requireId(g: GraphStore, id?: string): string {
	const resolved = id ?? g.load().focused_id;
	if (!resolved) throw new Error("Nothing is focused. Call cymose_branch or cymose_focus.");
	return resolved;
}

function textOut() {
	return {
		schema: { type: "string" as const },
		render: (_args: unknown, value: string) => [{ type: "text" as const, text: value }],
	};
}

export function apply(ctx: Context, config: Config): void {
	const prompts = ctx.get("systemPrompt") as
		| { section: (section: { name: string; order: number; text: () => string }) => void }
		| undefined;
	prompts?.section({
		name: "cymose:map",
		order: 45,
		text: () => store(config).focusedContext(),
	});

	ctx.tools.register(
		defineTool({
			name: "cymose_tree",
			description:
				"Show the Cymose session graph: what was tried, what failed, what is focused. Call this to see context.",
			parameters: {},
			output: textOut(),
			async execute() {
				const g = store(config).load();
				return treeListing(g.nodes, g.focused_id);
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_branch",
			description:
				"Start a new Cymose node (a short session). Children inherit ancestor summaries. Omit parent_id to fork from the focused node, or to create a root if the graph is empty.",
			parameters: {
				title: { type: "string", required: true, description: "What this session is trying to do" },
				parent_id: { type: "string", description: "Parent node id. Default: currently focused node." },
			},
			output: textOut(),
			async execute(args: { title: string; parent_id?: string }) {
				const g = store(config);
				const loaded = g.load();
				let parent: string | null;
				if (args.parent_id) parent = args.parent_id;
				else if (loaded.focused_id) parent = loaded.focused_id;
				else parent = null;
				const node = g.branch(args.title, parent);
				return `Created and focused ${node.id}.\n\n${g.context(node.id)}`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_focus",
			description: "Set the active Cymose node. Later inherit/summarize/promote calls use this node.",
			parameters: {
				id: { type: "string", required: true, description: "Node id from cymose_tree" },
			},
			output: textOut(),
			async execute(args: { id: string }) {
				const node = store(config).focus(args.id);
				return `Focused "${node.title}" (${node.id}), status ${node.status}.`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_inherit",
			description:
				"Return ancestor summaries for the focused node (or id). Read this before repeating an approach that already failed.",
			parameters: {
				id: { type: "string", description: "Node id. Default: focused node." },
			},
			output: textOut(),
			async execute(args: { id?: string }) {
				const g = store(config);
				const id = args.id ?? g.load().focused_id;
				if (!id) return "Nothing is focused. Call cymose_branch or cymose_focus.";
				return g.context(id);
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_mark",
			description:
				"Set a node's status. Use dead-end or failed when an approach should not be repeated by children.",
			parameters: {
				status: {
					type: "string",
					required: true,
					description: "active | done | failed | dead-end",
				},
				id: { type: "string", description: "Node id. Default: focused node." },
			},
			output: textOut(),
			async execute(args: { status: string; id?: string }) {
				const allowed: NodeStatus[] = ["active", "done", "failed", "dead-end"];
				if (!allowed.includes(args.status as NodeStatus)) {
					throw new Error(`status must be one of ${allowed.join(", ")}`);
				}
				const g = store(config);
				const id = requireId(g, args.id);
				const node = g.setStatus(id, args.status as NodeStatus);
				return `"${node.title}" is now ${node.status}.`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_summarize",
			description:
				"Write the summary children will inherit. You compose it here using this harness's model — nothing is sent to another service. Outcome is your verdict.",
			parameters: {
				task: { type: "string", required: true, description: "What this session set out to do" },
				outcome: {
					type: "string",
					required: true,
					description: "done | failed | unknown",
				},
				approach: { type: "string", required: true, description: "What was done, two or three sentences" },
				files_touched: {
					type: "string",
					description: "Comma-separated paths this session changed",
				},
				key_decisions: {
					type: "string",
					description: "Choices a later session should keep; semicolon-separated",
				},
				errors_encountered: {
					type: "string",
					description: "What went wrong and why; semicolon-separated",
				},
				id: { type: "string", description: "Node id. Default: focused node." },
			},
			output: textOut(),
			async execute(args: {
				task: string;
				outcome: string;
				approach: string;
				files_touched?: string;
				key_decisions?: string;
				errors_encountered?: string;
				id?: string;
			}) {
				const g = store(config);
				const id = requireId(g, args.id);
				const text = formatSummary({
					task: args.task,
					outcome: args.outcome,
					approach: args.approach,
					files_touched: splitList(args.files_touched, ","),
					key_decisions: splitList(args.key_decisions, ";"),
					errors_encountered: splitList(args.errors_encountered, ";"),
				});
				g.setSummary(id, text);
				if (args.outcome === "failed") g.setStatus(id, "failed");
				else if (args.outcome === "done") g.setStatus(id, "done");
				return `Stored summary on ${id}:\n\n${text}`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_explore",
			description:
				"Fork the focused node (or parent_id) into several sibling approaches. You invent the titles; each child inherits the same ancestors.",
			parameters: {
				titles: {
					type: "string",
					required: true,
					description: "Two or more approach titles, separated by |",
				},
				parent_id: { type: "string", description: "Parent node. Default: focused node." },
			},
			output: textOut(),
			async execute(args: { titles: string; parent_id?: string }) {
				const g = store(config);
				const parent = requireId(g, args.parent_id);
				const titles = args.titles
					.split("|")
					.map((t) => t.trim())
					.filter(Boolean);
				const nodes = g.explore(parent, titles);
				return ["Forked approaches (parent stays focused):", ...nodes.map((n) => `- ${n.title} ${n.id}`), "", g.focusedContext()].join(
					"\n",
				);
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_diff",
			description: "Show two nodes' summaries side by side so you can compare approaches.",
			parameters: {
				a: { type: "string", required: true, description: "First node id" },
				b: { type: "string", required: true, description: "Second node id" },
			},
			output: textOut(),
			async execute(args: { a: string; b: string }) {
				const g = store(config);
				const left = g.get(args.a);
				const right = g.get(args.b);
				if (!left) throw new Error(`No node ${args.a}.`);
				if (!right) throw new Error(`No node ${args.b}.`);
				return diffText(left, right);
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_combine",
			description:
				"Write a synthesis onto the target node. You write the takeaway using this harness; it is stored locally.",
			parameters: {
				target_id: { type: "string", required: true, description: "Node that receives the synthesis" },
				takeaway: { type: "string", required: true, description: "The combined conclusion" },
			},
			output: textOut(),
			async execute(args: { target_id: string; takeaway: string }) {
				const node = store(config).setSummary(args.target_id, args.takeaway.trim());
				return `Combined takeaway stored on "${node.title}" (${node.id}).`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_promote",
			description:
				"Fold this node's summary (or an explicit conclusion) onto its parent so the parent sees the outcome.",
			parameters: {
				id: { type: "string", description: "Child node. Default: focused node." },
				conclusion: { type: "string", description: "Override the stored summary" },
			},
			output: textOut(),
			async execute(args: { id?: string; conclusion?: string }) {
				const g = store(config);
				const id = requireId(g, args.id);
				const parent = g.promote(id, args.conclusion);
				return `Promoted onto "${parent.title}" (${parent.id}):\n\n${parent.promoted}`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_pick",
			description:
				"Copy summaries from other nodes onto the target, labeled by origin. Use this when an exact wording or a failed-path note must travel, not a new synthesis.",
			parameters: {
				target_id: { type: "string", required: true, description: "Node that receives the copies" },
				source_ids: {
					type: "string",
					required: true,
					description: "Source node ids, separated by comma",
				},
			},
			output: textOut(),
			async execute(args: { target_id: string; source_ids: string }) {
				const ids = args.source_ids
					.split(",")
					.map((s) => s.trim())
					.filter(Boolean);
				const node = store(config).pick(args.target_id, ids);
				return `Copied onto "${node.title}" (${node.id}).\n\n${node.summary}`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_dump",
			description:
				"Return the graph as JSON. Ask the harness to keep that JSON in the workspace if you want it after this process ends. This plugin does not touch the disk.",
			parameters: {},
			output: textOut(),
			async execute() {
				return store(config).dump();
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_load",
			description:
				"Replace the in-process graph with JSON previously shown by cymose_dump (or read by the harness from the workspace).",
			parameters: {
				json: { type: "string", required: true, description: "Graph JSON" },
			},
			output: textOut(),
			async execute(args: { json: string }) {
				const g = store(config);
				g.restore(args.json);
				const snap = g.load();
				return treeListing(snap.nodes, snap.focused_id);
			},
		}),
	);
}

function splitList(value: string | undefined, sep: string): string[] {
	if (!value?.trim()) return [];
	return value
		.split(sep)
		.map((s) => s.trim())
		.filter(Boolean);
}
