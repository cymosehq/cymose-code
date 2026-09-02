import type { Context } from "@deepseek-ai/cordis";
import { defineTool } from "@deepseek-ai/dsh-tools";
import { GraphStore } from "../src/graph.js";
import { runTool, toolSpecs } from "../src/tools.js";

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
	const created = new GraphStore("dsh");
	graphs.set(key, created);
	return created;
}

function textOut() {
	return {
		schema: { type: "string" as const },
		render: (_args: unknown, value: string) => [{ type: "text" as const, text: value }],
	};
}

function dshParameters(spec: (typeof toolSpecs)[number]) {
	const required = new Set(spec.inputSchema.required ?? []);
	const parameters: Record<string, { type: "string"; required?: true; description: string }> = {};
	for (const [key, field] of Object.entries(spec.inputSchema.properties)) {
		parameters[key] = {
			type: "string",
			description: field.description,
			...(required.has(key) ? { required: true as const } : {}),
		};
	}
	return parameters;
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

	for (const spec of toolSpecs) {
		ctx.tools.register(
			defineTool({
				name: spec.name,
				description: spec.description,
				parameters: dshParameters(spec),
				output: textOut(),
				async execute(args: Record<string, unknown>) {
					return runTool(store(config), spec.name, args);
				},
			}),
		);
	}
}
