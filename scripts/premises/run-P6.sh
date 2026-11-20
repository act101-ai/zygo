#!/bin/sh
# P6: act-lite. Latest release binary + checksum into a scratch directory (never onto PATH),
# checksum verified, run with the isolated HOME. P6a: `act-lite status` machine-readable
# language/extension list. P6b: `act-lite mcp serve` tools include skeleton, symbols,
# symbols_batch, repo_outline.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/act-lite"
rm -rf "$FX"; mkdir -p "$FX"
new_run P6
DL="$RUN/dl"; X="$RUN/x"; mkdir -p "$DL" "$X"
curl -sS --max-time 30 -o "$DL/latest.json" https://api.github.com/repos/act101-ai/act-lite/releases/latest
TAG=$(jq -r .tag_name "$DL/latest.json")
case "$(uname -m)" in arm64|aarch64) T=aarch64-apple-darwin ;; *) T=x86_64-apple-darwin ;; esac
A="act-lite-$TAG-$T.tar.gz"
curl -sSL --max-time 120 -o "$DL/$A" "https://github.com/act101-ai/act-lite/releases/download/$TAG/$A"
curl -sSL --max-time 30 -o "$DL/$A.sha256" "https://github.com/act101-ai/act-lite/releases/download/$TAG/$A.sha256"
want=$(awk '{print $1}' "$DL/$A.sha256")
got=$(shasum -a 256 "$DL/$A" | awk '{print $1}')
[ "$want" = "$got" ] || { echo "checksum mismatch"; exit 1; }
tar -xzf "$DL/$A" -C "$X"
BIN=$(find "$X" -type f -name act-lite | head -n 1)
al() { (cd "$R" && env -i PATH=/usr/bin:/bin HOME="$H" XDG_CONFIG_HOME="$H/.config" XDG_DATA_HOME="$H/.local/share" XDG_CACHE_HOME="$H/.cache" "$BIN" "$@" </dev/null); }
VER=$(al --version 2>&1 | head -n 1)
echo "act-lite: tag $TAG, --version: $VER, sha256 $got"
al --help >"$RUN/help.txt" 2>&1 || true
al status --help >"$RUN/status-help.txt" 2>&1 || true
al status >"$RUN/status.txt" 2>&1 || true
for f in --json "--format json" "--output json" -o\ json; do
  al status $f >"$RUN/status-try.out" 2>&1 && python3 -I -c 'import json,sys; json.load(open(sys.argv[1]))' "$RUN/status-try.out" 2>/dev/null && { cp "$RUN/status-try.out" "$RUN/status.json"; echo "status form: act-lite status $f"; echo "$f" >"$RUN/status-flag"; break; }
done
(cd "$R" && env -i PATH=/usr/bin:/bin HOME="$H" XDG_CONFIG_HOME="$H/.config" XDG_DATA_HOME="$H/.local/share" XDG_CACHE_HOME="$H/.cache"   python3 -I "$PREMISES_DIR/mcp_list.py" --call status "$BIN" mcp serve) >"$RUN/mcp-tools.json" 2>"$RUN/mcp.err" || true
# P6b: the four navigation tools in the tool list.
python3 -I - "$RUN/mcp-tools.json" >"$RUN/p6b.txt" <<'PY'
import json, sys
names = {t["name"] for t in json.load(open(sys.argv[1]))["tools"]}
need = ["skeleton", "symbols", "symbols_batch", "repo_outline"]
print("P6b", "verified" if all(n in names for n in need) else "failed", "missing:", [n for n in need if n not in names])
PY
cat "$RUN/p6b.txt"
# P6a fallback data: the extension list of this release, from its source (LanguageMeta per language).
SRC="$RUN/src"; mkdir -p "$SRC"
curl -sS --max-time 30 -o "$SRC/tree.json" "https://api.github.com/repos/act101-ai/act-lite/git/trees/$TAG?recursive=1"
for f in $(jq -r '.tree[].path' "$SRC/tree.json" | grep '^crates/act-lite-parser/src/languages/[a-z_]*\.rs$'); do
  curl -sSL --max-time 30 -o "$SRC/$(basename "$f")" "https://raw.githubusercontent.com/act101-ai/act-lite/$TAG/$f"
done
curl -sSL --max-time 30 -o "$SRC/README.md" "https://raw.githubusercontent.com/act101-ai/act-lite/$TAG/README.md"
python3 -I - "$SRC" "$TAG" "$RUN/mcp-tools.json" >"$RUN/languages.json" <<'PY'
import glob, json, os, re, sys
src, tag, tools = sys.argv[1:4]
langs = []
for f in sorted(glob.glob(os.path.join(src, "*.rs"))):
    t = open(f).read()
    for m in re.finditer(r'LanguageMeta\s*\{\s*id:\s*LanguageId\("([^"]+)"\),\s*display_name:\s*"([^"]+)",\s*extensions:\s*&\[([^\]]*)\]', t):
        langs.append({"language": m.group(2), "id": m.group(1), "extensions": re.findall(r'"([^"]*)"', m.group(3)),
                      "source": "https://github.com/act101-ai/act-lite/blob/%s/crates/act-lite-parser/src/languages/%s" % (tag, os.path.basename(f))})
# File names recognized before extensions (README "Languages" table, same tag).
names, on = {}, False
for line in open(os.path.join(src, "README.md")):
    line = line.strip()
    if line == "| Language | File names | Extensions |":
        on = True
        continue
    if on and not line.startswith("|"):
        break
    m = re.match(r"^\| ([^|]+) \| ([^|]+) \| ([^|]+) \|$", line)
    if on and m and not m.group(1).startswith("---"):
        names[m.group(1).strip()] = [] if m.group(2).strip() == "none" else re.findall(r"`([^`]+)`", m.group(2))
for l in langs:
    if names.get(l["language"]):
        l["file_names"] = names[l["language"]]
st = json.load(open(tools)).get("call", {}).get("result", {})
status_langs = json.loads(st["content"][0]["text"])["languages"] if st else []
print(json.dumps({"act_lite": tag, "retrieved_from": "https://github.com/act101-ai/act-lite/tree/%s" % tag,
                  "file_names_from": "https://github.com/act101-ai/act-lite/blob/%s/README.md#languages" % tag,
                  "languages_count": len(langs),
                  "same_names_as_mcp_status_tool": sorted(l["language"] for l in langs) == sorted(status_langs),
                  "languages": langs}, indent=1))
PY
{ echo "tag: $TAG"; echo "asset: $A"; echo "sha256: $got (matches $A.sha256)"; echo "--version: $VER"; } >"$RUN/version.txt"
for f in help.txt status-help.txt status.txt version.txt; do scrub "$RUN/$f" >"$FX/$f"; done
if [ -s "$RUN/status.json" ]; then
  scrub "$RUN/status.json" >"$FX/status.json"
else
  scrub "$RUN/languages.json" >"$FX/languages.json"
  cat >"$FX/README.md" <<R
# act-lite $TAG: P6a failed, P6b verified

- P6a (\`act-lite status\` machine-readable language list): **failed**. The release has no
  \`status\` subcommand (\`status.txt\`: "unrecognized subcommand 'status'"; \`help.txt\` lists
  query, mcp, env, analyze). Its MCP server has a \`status\` tool (\`mcp-tools.json\`, "call")
  that returns JSON with language *names* only, no extensions, and it needs a running MCP
  server. Fallback (spec §19 P6): zygo compiles the extension list of this release,
  \`languages.json\`, taken from the release source (each language's \`LanguageMeta\`) and
  README (file names), with URLs.
- P6b (MCP tool names): **verified**. \`act-lite mcp serve\` lists skeleton, symbols,
  symbols_batch and repo_outline (\`mcp-tools.json\`).
- Release asset and checksum: \`version.txt\`.
R
fi
[ -s "$RUN/mcp-tools.json" ] && scrub "$RUN/mcp-tools.json" >"$FX/mcp-tools.json"
cat "$RUN/status-help.txt"; head -40 "$RUN/status.txt"; jq -c '[.serverInfo, [.tools[].name]]' "$RUN/mcp-tools.json"
echo "P6 done"
