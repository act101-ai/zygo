# act-lite v1.2.3: P6a failed, P6b verified

- P6a (`act-lite status` machine-readable language list): **failed**. The release has no
  `status` subcommand (`status.txt`: "unrecognized subcommand 'status'"; `help.txt` lists
  query, mcp, env, analyze). Its MCP server has a `status` tool (`mcp-tools.json`, "call")
  that returns JSON with language *names* only, no extensions, and it needs a running MCP
  server. Fallback (spec §19 P6): zygo compiles the extension list of this release,
  `languages.json`, taken from the release source (each language's `LanguageMeta`) and
  README (file names), with URLs.
- P6b (MCP tool names): **verified**. `act-lite mcp serve` lists skeleton, symbols,
  symbols_batch and repo_outline (`mcp-tools.json`).
- Release asset and checksum: `version.txt`.
