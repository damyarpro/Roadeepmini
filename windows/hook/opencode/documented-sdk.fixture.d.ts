// Contract fixture from https://opencode.ai/v2/docs/build/plugins/.
// This checks our use of the documented shape; it is not a substitute for host-version integration testing.
declare module "@opencode/plugin" {
  interface Registration { dispose(): Promise<void> | void }
  interface Metadata { readonly sessionID: string; readonly tool?: string; readonly callID?: string; readonly id?: string }
  interface Context {
    readonly location: { readonly directory: string };
    readonly session: { hook(name: "prompt", callback: (event: Metadata & { prompt: { text: string } }) => Promise<void> | void): Promise<Registration> };
    readonly tool: {
      hook(name: "execute.before", callback: (event: Metadata & { input: Record<string, unknown> }) => Promise<void> | void): Promise<Registration>;
      hook(name: "execute.after", callback: (event: Metadata & { status: "completed" | "error" }) => Promise<void> | void): Promise<Registration>;
    };
    readonly event: { subscribe(options: { signal: AbortSignal }): AsyncIterable<{ type: string; properties?: unknown; data?: unknown }> };
  }
  export const Plugin: { define<T extends { id: string; setup(ctx: Context): Promise<void | (() => Promise<void>)> }>(plugin: T): T };
}
