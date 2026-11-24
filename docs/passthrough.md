# Passthrough

`zygo claude --passthrough` runs Claude Code on your claude.ai subscription login and model choice. Redaction, the result store, the soft context window, cache handling, hooks, MCP tools, plugins, work units and the session text still apply (spec §12.1).

## Risk and local mode only

The vendor's consumer terms restrict routing subscription credentials through third-party software. Your own account carries that risk.

Passthrough works in local mode only. The login goes from Claude Code to the in-process proxy on loopback, and from there to the vendor. It never passes through a shared server. Serve mode refuses a passthrough session with `403 zygo_passthrough_local_only`.

## Credential type

| Type | Agent | Upstream host | Forwarded credential (allowlist) |
|---|---|---|---|
| `claude-ai` | Claude Code | `api.anthropic.com` | `Authorization: Bearer sk-ant-oat…` only |

The proxy reads this row; it holds no other values for the type.

## Proxy behaviour

- **Identity.** On `/anthropic/*`, when the `X-Zygo-Token` header is present, it carries the session token, with `X-Zygo-Session`. The session must be a passthrough session. `Authorization` is not read as identity. An invalid or empty token gives `401 zygo_passthrough_token_invalid` "relaunch zygo claude --passthrough".
- **Login check.** The request must carry exactly one `Authorization` header and no `x-api-key`. Its value is `Bearer`, one space and one token that starts with `sk-ant-oat`, with more after the prefix and no space or comma. Anything else (no login, `x-api-key`, a `zyg_` token, any other bearer) gives `401 zygo_passthrough_no_login` "passthrough needs a claude.ai login (/login)". Nothing is sent upstream.
- **Model.** No role resolution. The model is sent as received, `[1m]` included. The first catalog model with a native Anthropic provider whose `upstream_model` equals the name (without `[1m]`) supplies the model facts. The role is recorded as `subscription`.
- **Upstream.** One candidate: the matched model's provider on `api.anthropic.com` port 443, else a built-in provider for that host with no key. The login is sent only to `api.anthropic.com` on port 443; the same host on another port does not get it. No provider key is read; a provider with `enabled = false` is still dropped, and `require_zdr` still applies.
- **Headers.** The client's `Authorization` and `anthropic-beta` are forwarded unchanged. No `x-api-key` is added. Every `x-zygo-*` and `x-claude-code-*` header is removed.
- **Usage.** The usage row records the tokens, `role = subscription` and a cost estimate of 0. The login is never logged or stored.

## Limits

- Claude Code reads the session token (in `ANTHROPIC_CUSTOM_HEADERS`) once at start. The session has no idle expiry: its token stays valid while its launcher runs.
- A `/model` switch mid-session does not move the lead guard.
- `GET /anthropic/v1/models` still lists the role names. Role names do not work in passthrough: use a vendor model id.

## Several logins on one machine

Each Claude Code config directory holds its own claude.ai login. `--config-dir` sets `CLAUDE_CONFIG_DIR` for the launch, so one machine can run several subscription logins side by side:

```
zygo claude --passthrough --config-dir ~/.claude-interns/<name>
```

The directory must exist. Run `/login` once in each directory. The flag wins over an inherited `CLAUDE_CONFIG_DIR`.
