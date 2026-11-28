import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";

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
  let chain = Promise.resolve("");
  // One call after another, in decision order; `run` never rejects, so the chain cannot break.
  const ws = (event, payload) => (chain = chain.then(() => run("workspace", payload, ["--event", event])));
  // The factory runs only in a spawned agent: this call commits the pending start (AU92, AU104).
  if (member) ws("session", {});
  const sessionOf = (e) => {
    const p = e.properties || {};
    const i = p.info || {};
    return p.sessionID || i.sessionID || (String(e.type).startsWith("session.") ? i.id : undefined) || (p.part || {}).sessionID || null;
  };
  const statusOf = (e) => {
    const s = (e.properties || {}).status;
    return s && typeof s === "object" ? s.type : s;
  };
  // Session kinds (`top` or `child`, only from the `info` of a `session.*` event, first value final), activation
  // numbers from one counter, the main session (the known top session activated last), the id of the last
  // `session` call with an id, the deleted sessions (never the main session), and the turn set (sessions in a turn
  // that are not known children).
  const kind = new Map();
  const act = new Map();
  let n = 0;
  let main = null;
  let mainAct = 0;
  let sent = null;
  const deleted = new Set();
  const inTurn = new Set();
  // AU27 tool count: the counted tool calls, each one `+1` in zygo's count, keyed by session id and `callID`
  // together. A call leaves at its `tool.execute.after`, or when the plugin sends `prompt` or `stop`, since zygo
  // then sets the one member count to 0.
  const counted = new Set();
  const callKey = (sessionID, callID) => `${sessionID}\u0000${callID}`;
  const isId = (s) => typeof s === "string" && s !== "";
  // A part id of the OpenCode 1.19.6 form: `prt_`, the low 48 bits of `ms * 4096 + counter` as 12 hex digits (here
  // the highest counter of the millisecond, so it sorts after the parts OpenCode made before the hook), 14 base62.
  const B62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
  const partId = () => {
    const t = (BigInt(Date.now()) * 4096n + 4095n).toString(16).padStart(12, "0").slice(-12);
    let r = "";
    for (const b of randomBytes(14)) r += B62[b % 62];
    return `prt_${t}${r}`;
  };
  const leave = (id) => {
    if (inTurn.delete(id) && inTurn.size === 0) {
      ws("stop", { sessionID: id });
      counted.clear();
    }
  };
  const take = (id, a) => {
    main = id;
    mainAct = a;
    if (id !== sent) {
      sent = id;
      ws("session", { sessionID: id });
    }
  };
  const consider = (id) => {
    const a = act.get(id);
    if (kind.get(id) !== "top" || deleted.has(id) || a === undefined || (main !== null && a <= mainAct)) return;
    take(id, a);
  };
  // The main session was deleted: the known, not deleted top session with the highest activation, or none.
  const reselect = () => {
    let best = null;
    let bestAct = 0;
    for (const [id, a] of act) {
      if (kind.get(id) === "top" && !deleted.has(id) && a > bestAct) {
        best = id;
        bestAct = a;
      }
    }
    if (best === null) {
      main = null;
      mainAct = 0;
    } else {
      take(best, bestAct);
    }
  };
  // AU27, W§9.1. Decides and queues before any wait (D11): (1) classify from `info` (a present, non-null
  // `parentID` makes a child); a new child leaves the turn set; a `session.deleted` marks its session deleted and
  // it leaves the turn set; a deleted main session gives the main session to the newest known top session;
  // (2) `session.created` or a `busy` status activates; (3) the turn set sends `prompt` when it becomes non-empty
  // and `stop` when it becomes empty; (4) a known top session that is not deleted and was activated later than the
  // main session becomes the main session and is sent once.
  const step = (e) => {
    const id = sessionOf(e);
    const p = e.properties || {};
    const info = p.info && typeof p.info === "object" ? p.info : null;
    const infoId = String(e.type).startsWith("session.") && info && isId(info.id) ? info.id : null;
    if (infoId !== null && !kind.has(infoId)) {
      const child = info.parentID !== undefined && info.parentID !== null;
      kind.set(infoId, child ? "child" : "top");
      if (child) leave(infoId);
    }
    if (e.type === "session.deleted" && infoId !== null) {
      deleted.add(infoId);
      leave(infoId);
      if (infoId === main) reselect();
    }
    const busy = e.type === "session.status" && statusOf(e) === "busy";
    const idle = e.type === "session.idle" || (e.type === "session.status" && statusOf(e) === "idle");
    if (e.type === "session.created" && infoId !== null) act.set(infoId, ++n);
    if (busy && isId(id)) act.set(id, ++n);
    if (busy && isId(id) && kind.get(id) !== "child" && !inTurn.has(id)) {
      inTurn.add(id);
      if (inTurn.size === 1) {
        ws("prompt", { sessionID: id });
        counted.clear();
      }
    } else if (idle && isId(id)) {
      leave(id);
    }
    for (const s of new Set([id, infoId])) if (isId(s)) consider(s);
  };
  return {
    // Does not wait for the queued calls.
    event: async ({ event }) => {
      if (member && event) step(event);
    },
    // AU27 tool count: `task` waits for its `Agent` call in every session (a deny throws); every other tool of a
    // session that is not a known child queues its `pre-tool` and does not wait. A call is counted by session id
    // and `callID` when both are non-empty strings, when its `pre-tool` is queued, before any wait; a deny answer
    // to the `task` call forgets it (a `prompt` or `stop` queued during the wait has already forgotten it).
    "tool.execute.before": async (input, output) => {
      if (member && input && input.tool === "task") {
        const key = isId(input.sessionID) && isId(input.callID) ? callKey(input.sessionID, input.callID) : null;
        const answer = ws("pre-tool", { sessionID: input.sessionID, tool_name: "Agent" });
        if (key !== null) counted.add(key);
        const w = hookOutput(await answer);
        if (w && w.permissionDecision === "deny") {
          if (key !== null) counted.delete(key);
          throw new Error(w.permissionDecisionReason);
        }
      } else if (member && input && isId(input.sessionID) && kind.get(input.sessionID) !== "child") {
        ws("pre-tool", { sessionID: input.sessionID, tool_name: input.tool });
        if (isId(input.callID)) counted.add(callKey(input.sessionID, input.callID));
      }
      const h = hookOutput(await run("guard", { input, output }));
      if (h && h.permissionDecision === "deny") throw new Error(h.permissionDecisionReason);
    },
    // A counted call queues its one `tool-end` first, before any wait; no kind check here (AU27 tool count).
    "tool.execute.after": async (input, output) => {
      if (member && input && isId(input.sessionID) && isId(input.callID)) {
        const key = callKey(input.sessionID, input.callID);
        if (counted.delete(key)) ws("tool-end", { sessionID: input.sessionID });
      }
      const h = hookOutput(await run("trim", { input, output }));
      if (h && h.updatedToolOutput && typeof h.updatedToolOutput.stdout === "string") {
        output.output = h.updatedToolOutput.stdout;
      }
      await run("work", { input, output });
    },
    // T§7.2 item 1: every `zygo/` model; the main role as today, any other role (or no main role) `--event subagent`.
    "experimental.chat.system.transform": async (input, output) => {
      if (!input.model || input.model.providerID !== "zygo" || !output || !Array.isArray(output.system)) return;
      const role = process.env.ZYGO_MAIN_ROLE;
      const extra = role && input.model.id === role ? [] : ["--event", "subagent"];
      const text = await run("session-start", { input, output }, extra);
      if (text) output.system.push(text);
    },
    // M§7.2 (FX-33): a user message of the main session (the member gate, the main role's `zygo/` model, not a known
    // child) gets the mail line as one synthetic text part, so the line is in the saved user turn and the system text
    // of each request does not change with the unread count. The call changes no state, so it does not wait for the
    // queued member calls.
    "chat.message": async (input, output) => {
      const role = process.env.ZYGO_MAIN_ROLE;
      const m = output && output.message && typeof output.message === "object" ? output.message : null;
      const model = m && m.model && typeof m.model === "object" ? m.model : null;
      if (!member || !input || !isId(input.sessionID) || kind.get(input.sessionID) === "child") return;
      if (!model || model.providerID !== "zygo" || !role || model.modelID !== role || !Array.isArray(output.parts)) return;
      const line = await run("workspace", { sessionID: input.sessionID }, ["--event", "message"]);
      if (line) {
        output.parts.push({ id: partId(), sessionID: input.sessionID, messageID: m.id, type: "text", text: line, synthetic: true });
      }
    },
    "experimental.session.compacting": async (input, output) => {
      const text = await run("pre-compact", { input, output });
      if (text) output.context.push(text);
    },
  };
};
