import type { Context } from "@deepseek-ai/cordis";
import Schema from "@deepseek-ai/schemastery";
import { defineTool } from "@deepseek-ai/dsh-tools";
import { CymoseClient } from "../src/api.js";
import { GraphStore } from "../src/graph.js";
import { formatSummary, inheritText, treeListing } from "../src/inherit.js";
import { defaultGraphPath, deviceId } from "../src/paths.js";
import type { NodeStatus } from "../src/ir.js";

export const name = "cymose";

export const inject = ["tools"];

export interface Config {
	apiUrl: string;
	token: string;
	/** Override the per-workspace graph file. Default: `<cwd>/.cymose/graph.json`. */
	graphPath?: string;
}

export const Config: Schema<Config> = Schema.object({
	apiUrl: Schema.string().default("https://api.cymose.app"),
	token: Schema.string().default(""),
	graphPath: Schema.string().default(""),
});

function store(config: Config): GraphStore {
	const path = config.graphPath?.trim() || defaultGraphPath();
	return new GraphStore(path);
}

function client(config: Config): CymoseClient {
	const token = config.token.trim();
	if (!token) {
		throw new Error(
			"No Cymose token. Create one at web.cymose.app → Settings → Connected apps, then set `token` on the cymose plugin config.",
		);
	}
	return new CymoseClient(config.apiUrl, token, deviceId());
}

function textOut() {
	return {
		schema: { type: "string" as const },
		render: (_args: unknown, value: string) => [{ type: "text" as const, text: value }],
	};
}

export function apply(ctx: Context, config: Config): void {
	ctx.tools.register(
		defineTool({
			name: "cymose_tree",
			description:
				"Show the Cymose session graph for this workspace: what was tried, what failed, what is focused. Call this when you need to see context, not to save tokens.",
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
				"Start a new Cymose node (a short session). Children inherit ancestor summaries. Pass parent_id to fork; omit it to fork from the focused node, or to create a root if the graph is empty.",
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
				const text = inheritText(node, g.ancestors(node.id));
				return `Created and focused ${node.id}.\n\n${text}`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_focus",
			description: "Set the active Cymose node. Later inherit/summarize calls use this node.",
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
				const node = g.get(id);
				if (!node) throw new Error(`No node ${id}.`);
				return inheritText(node, g.ancestors(id));
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
				const id = args.id ?? g.load().focused_id;
				if (!id) throw new Error("Nothing is focused.");
				const node = g.setStatus(id, args.status as NodeStatus);
				return `"${node.title}" is now ${node.status}.`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_summarize",
			description:
				"Compress this session into the summary children will inherit. Needs a Cymose token. Pass the transcript of this coding session; the client's outcome (done/failed) wins over the model.",
			parameters: {
				task: { type: "string", required: true, description: "What this session set out to do" },
				outcome: {
					type: "string",
					required: true,
					description: "done | failed | unknown — your verdict, not the model's",
				},
				transcript: {
					type: "string",
					required: true,
					description: "Plain-text transcript (role-prefixed lines are fine)",
				},
				id: { type: "string", description: "Node id. Default: focused node." },
			},
			output: textOut(),
			async execute(args: { task: string; outcome: string; transcript: string; id?: string }) {
				const g = store(config);
				const id = args.id ?? g.load().focused_id;
				if (!id) throw new Error("Nothing is focused.");
				const summary = await client(config).summarize({
					task: args.task,
					outcome: args.outcome,
					transcript: [{ role: "user", content: args.transcript }],
				});
				const text = formatSummary(summary);
				g.setSummary(id, text);
				if (args.outcome === "failed") g.setStatus(id, "failed");
				else if (args.outcome === "done") g.setStatus(id, "done");
				return `Stored summary on ${id}:\n\n${text}`;
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_sync",
			description:
				"Read the Cymose Web canvas tree (read-only). Use this to see the plan you sketched in the browser.",
			parameters: {},
			output: textOut(),
			async execute() {
				const tree = await client(config).syncTree();
				if (!tree.nodes?.length) return "The web tree is empty.";
				return tree.nodes
					.map((n) => {
						const bits = [n.title, n.id];
						if (n.inherited_summary) bits.push(`inherited: ${n.inherited_summary}`);
						if (n.promoted_digest) bits.push(`promoted: ${n.promoted_digest}`);
						return `- ${bits.join(" · ")}`;
					})
					.join("\n");
			},
		}),
	);

	ctx.tools.register(
		defineTool({
			name: "cymose_whoami",
			description: "Check that the Cymose token works and show credit allowance.",
			parameters: {},
			output: textOut(),
			async execute() {
				const body = await client(config).credits();
				return JSON.stringify(body, null, 2);
			},
		}),
	);
}
