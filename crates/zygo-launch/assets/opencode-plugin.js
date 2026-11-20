import { spawn } from "node:child_process";

const ZYGO = __ZYGO_PATH__;

// Runs `zygo hook <name> --agent opencode` with `payload` on stdin; resolves "" on any failure.
export const ZygoPlugin = async ({ directory }) => {
  const run = (name, payload) =>
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
        child = spawn(ZYGO, ["hook", name, "--agent", "opencode"], { stdio: ["pipe", "pipe", "ignore"] });
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
  return {
    "tool.execute.before": async (input, output) => {
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
