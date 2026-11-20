// N2 probe plugin for OpenCode: logs the first input/output of each hook (per tool for the tool
// hooks) to $N2_OC_LOG, replaces the bash tool's output, adds a system text line and a pre-compaction context line.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";

const dir = process.env.N2_OC_LOG;
function log(name, input, output) {
  try {
    mkdirSync(dir, { recursive: true });
    const f = `${dir}/${name}.json`;
    if (!existsSync(f)) writeFileSync(f, JSON.stringify({ input, output }, null, 2));
  } catch {}
}
const copy = (v) => JSON.parse(JSON.stringify(v ?? null));

export const ZygoProbe = async (ctx) => {
  log("plugin-init", { contextKeys: Object.keys(ctx).sort() }, null);
  return {
    "tool.execute.before": async (input, output) => log(`tool.execute.before-${input.tool}`, copy(input), copy(output)),
    "tool.execute.after": async (input, output) => {
      log(`tool.execute.after-${input.tool}`, copy(input), copy(output));
      if (input.tool === "bash") output.output = "OC-REPLACED-bash";
    },
    "experimental.chat.system.transform": async (input, output) => {
      output.system.push("tracer-oc-session-context");
      log("experimental.chat.system.transform", { modelID: input?.model?.id, providerID: input?.model?.providerID }, { systemEntries: output.system.length });
    },
    "experimental.session.compacting": async (input, output) => {
      log("experimental.session.compacting", copy(input), copy(output));
      if (Array.isArray(output.context)) output.context.push("tracer-oc-compaction-context");
    },
  };
};
