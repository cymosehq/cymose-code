import type { SessionSummary } from "./inherit.js";

export class ApiError extends Error {
	constructor(
		readonly status: number,
		message: string,
	) {
		super(message);
		this.name = "ApiError";
	}
}

export type TranscriptTurn = {
	role: string;
	content?: string;
	tool_calls?: { name: string; input: unknown }[];
};

export type SyncNode = {
	id: string;
	parent_id: string | null;
	title: string;
	inherited_summary: string | null;
	promoted_digest: string | null;
};

export type SyncTree = { version: number; nodes: SyncNode[] };

export class CymoseClient {
	constructor(
		private readonly baseUrl: string,
		private readonly token: string,
		private readonly deviceId: string,
	) {}

	private url(path: string): string {
		return `${this.baseUrl.replace(/\/+$/, "")}${path}`;
	}

	private headers(): Record<string, string> {
		return {
			Authorization: `Bearer ${this.token.trim()}`,
			"Content-Type": "application/json",
			"X-Cymose-Device": this.deviceId,
		};
	}

	async credits(): Promise<unknown> {
		const response = await fetch(this.url("/v1/credits"), { headers: this.headers() });
		return this.readJson(response);
	}

	async syncTree(): Promise<SyncTree> {
		const response = await fetch(this.url("/v1/sync/tree"), { headers: this.headers() });
		return this.readJson(response) as Promise<SyncTree>;
	}

	async summarize(body: {
		task: string;
		outcome: string;
		transcript: TranscriptTurn[];
	}): Promise<SessionSummary> {
		const response = await fetch(this.url("/v1/code/summarize"), {
			method: "POST",
			headers: this.headers(),
			body: JSON.stringify(body),
		});
		return this.readJson(response) as Promise<SessionSummary>;
	}

	private async readJson(response: Response): Promise<unknown> {
		const text = await response.text();
		let parsed: unknown = text;
		try {
			parsed = text ? JSON.parse(text) : {};
		} catch {
			parsed = { error: text };
		}
		if (!response.ok) {
			const err = parsed as { error?: string | { message?: string } };
			const message =
				typeof err.error === "string"
					? err.error
					: err.error && typeof err.error === "object"
						? (err.error.message ?? text)
						: text || `HTTP ${response.status}`;
			throw new ApiError(response.status, message);
		}
		return parsed;
	}
}
