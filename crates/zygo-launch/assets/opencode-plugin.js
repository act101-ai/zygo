import { spawn } from "node:child_process";

const ZYGO = __ZYGO_PATH__;

// Runs `zygo hook <name> --agent opencode [extra...]` with `payload` on stdin; resolves "" on any failure.
export const ZygoPlugin = async ({ directory }) => {
  const run = (name, payload, extra = []) =>
    new Promise((resolve) => {
      let out = "";
      let done = false;
      let child;
      const timer = setTimeout(() => {
        if (child) child.kill();
        finish("");
      }, 10000);
      const finish = (v) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        resolve(v);
      };
      try {
        child = spawn(ZYGO, ["hook", name, "--agent", "opencode", ...extra], { stdio: ["pipe", "pipe", "ignore"] });
        child.stdin.on("error", () => {});
        child.stdout.setEncoding("utf8");
        child.stdout.on("data", (d) => (out += d));
        child.on("error", () => finish(""));
        child.on("close", (code) => finish(code === 0 ? out : ""));
        child.stdin.end(JSON.stringify({ ...payload, cwd: directory }));
      } catch {
        finish("");
      }
    });
  const hookOutput = (text) => {
    try {
      return JSON.parse(text).hookSpecificOutput;
    } catch {
      return undefined;
    }
  };
  // Workspace member events (W§9.1, AU27, AU104). The gate is cheap in the agent process; hooks read the view marker (AU58).
  const member = typeof process.env.ZYGO_WORKSPACE === "string" && process.env.ZYGO_WORKSPACE !== "";
  let main = null;
  let idleNow = true;
  const children = new Set();
  let chain = Promise.resolve("");
  // One call after another, in decision order; `run` never rejects, so the chain cannot break.
  const ws = (event, payload) => (chain = chain.then(() => run("workspace", payload, ["--event", event])));
  const sessionOf = (e) => {
    const p = e.properties || {};
    const i = p.info || {};
    return p.sessionID || i.sessionID || (String(e.type).startsWith("session.") ? i.id : undefined) || (p.part || {}).sessionID || null;
  };
  const statusOf = (e) => {
    const s = (e.properties || {}).status;
    return s && typeof s === "object" ? s.type : s;
  };
  // Decides and queues before any wait (D11).
  const step = (e) => {
    const id = sessionOf(e);
    if (typeof id !== "string" || id === "") return;
    const info = (e.properties || {}).info || {};
    if (String(e.type).startsWith("session.") && info.parentID) {
      children.add(id);
      return;
    }
    if (children.has(id)) return;
    if (main === null || (e.type === "session.created" && id !== main)) {
      main = id;
      idleNow = true;
      ws("session", { sessionID: id });
    }
    if (id !== main) return;
    if (e.type === "session.status" && statusOf(e) === "busy" && idleNow) {
      idleNow = false;
      ws("prompt", { sessionID: id });
    } else if (e.type === "session.idle" || (e.type === "session.status" && statusOf(e) === "idle")) {
      idleNow = true;
      ws("stop", { sessionID: id });
    }
  };
  return {
    // Does not wait for the queued calls.
    event: async ({ event }) => {
      if (member && event) step(event);
    },
    "tool.execute.before": async (input, output) => {
      if (member && input && input.tool === "task") {
        const w = hookOutput(await ws("pre-tool", { sessionID: input.sessionID, tool_name: "Agent" }));
        if (w && w.permissionDecision === "deny") throw new Error(w.permissionDecisionReason);
      }
      const h = hookOutput(await run("guard", { input, output }));
      if (h && h.permissionDecision === "deny") throw new Error(h.permissionDecisionReason);
    },
    "tool.execute.after": async (input, output) => {
      const h = hookOutput(await run("trim", { input, output }));
      if (h && h.updatedToolOutput && typeof h.updatedToolOutput.stdout === "string") {
        output.output = h.updatedToolOutput.stdout;
      }
      await run("work", { input, output });
    },
    "experimental.chat.system.transform": async (input, output) => {
      const role = process.env.ZYGO_MAIN_ROLE;
      if (!role || !input.model || input.model.providerID !== "zygo" || input.model.id !== role) return;
      const text = await run("session-start", { input, output });
      if (text) output.system.push(text);
    },
    "experimental.session.compacting": async (input, output) => {
      const text = await run("pre-compact", { input, output });
      if (text) output.context.push(text);
    },
  };
};
