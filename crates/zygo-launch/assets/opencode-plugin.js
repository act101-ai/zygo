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
    // and `callID` when both are non-empty strings.
    "tool.execute.before": async (input, output) => {
      if (member && input && input.tool === "task") {
        const w = hookOutput(await ws("pre-tool", { sessionID: input.sessionID, tool_name: "Agent" }));
        if (w && w.permissionDecision === "deny") throw new Error(w.permissionDecisionReason);
        if (isId(input.sessionID) && isId(input.callID)) counted.add(callKey(input.sessionID, input.callID));
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
