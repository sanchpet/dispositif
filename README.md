# dispositif

dispositif lets a Claude Code agent answer Telegram messages when no Claude session is open. It is for people who already run [mcp-tg](https://github.com/lexfrei/mcp-tg) on a Telegram account set aside for their agent. It polls that account, passes through only the messages an allowlist admits, and answers each one with a single headless `claude -p` run. What the run is allowed to do depends on the rule that admitted the message.

It runs on macOS and Linux and needs an mcp-tg daemon serving MCP over streamable HTTP plus the `claude` CLI; the flags it passes exist in Claude Code 2.1.282. Telegram media (voice, photos, files) is not handled: such messages reach the run as a placeholder like `[photo]`.

## How it works

1. Each poll cycle opens one short MCP session to mcp-tg: `tg_dialogs_list`, then `tg_messages_list` for every dialog that has unread messages, is named by a rule, or has not been seen before. The session is closed with an HTTP `DELETE` at the end of the cycle.
2. Each new message is checked against the rules in order. The first rule that matches decides the message's trust tier. A message that no rule matches is skipped and never shown to a model.
3. An admitted message is marked read. It then gets a typing indicator, kept alive for the whole run, the last `history` messages of the chat as context, and one `claude -p --output-format json` run in the tier's working directory, with the tier's flags.
4. The run's final text is posted as a reply to that message, in that chat. If the run fails or times out, `fallback_reply` is posted instead. If the result is empty, nothing is posted.

When dispositif sees a chat for the first time, it skips messages dated before the process started and processes newer ones. So a restart with empty state does not answer old history, while a message that arrives in a new chat after start is still answered.

Claude sessions are kept per chat and tier (`peer:trust`). A new run resumes the previous session until that session is `session_ttl_secs` old.

## Threat model

- The allowlist runs before the model. Admission depends on the dialog peer, the sender id, and the trigger (any message, or a mention of the agent or a reply to it). A message no rule admits never starts a run. It can still reach a prompt as context: an admitted run is given the chat's last `history` messages, whoever wrote them, and the chat title. Lines from senders outside the allowlist are marked `(outside allowlist)` and the prompt tells the model to act only on the message it answers; that is an instruction to the model, not a boundary. So a run started in a group reads what every member wrote there, and the tier that handles it should be one you would give to the least trusted of them. Put a group's own rule, with a restricted tier, before any `peer = "*"` rule that maps the owner to an unrestricted tier, as the example config does; otherwise the owner mentioning the agent in a shared group starts an unrestricted run over text others planted.
- Tiers map trust to capabilities. A `restricted` tier runs with `--restricted --strict-mcp-config --tools <tools>`. That means no shell or other code-running tools, no MCP servers (so no Telegram access), file tools confined to the tier's `cwd`, user and project settings ignored, and `bypassPermissions` refused. `check` accepts only read-only tools in a restricted tier's comma-separated `tools` list (`Read`, `Grep`, `Glob`, `WebFetch`, `WebSearch`) and rejects `permission_mode = "bypassPermissions"` there. Anything else is refused because it runs code or writes files: a file written into a repository, such as a git hook, can run code later. An unrestricted tier runs with the full Claude Code configuration it finds through `claude_env` and `cwd`.
- The runner picks the reply chat, not the model. The reply always goes to the chat the message came from. A restricted run has no Telegram tools, so it cannot write anywhere else.

It does not protect against:

- A compromised allowlisted account. Whoever controls it gets that rule's tier.
- The prompt texts. `preamble`, tier `instructions` and `fallback_reply` come from your config verbatim, and nothing checks what they say.
- Anything an unrestricted tier is configured to do. `full` means your own Claude Code profile with its tools, MCP servers and permission mode.
- Reading. A restricted tier can still read everything under its `cwd` and fetch web pages if its `tools` allow it, and whatever it reads can end up in a reply.

## Install

From source (Rust toolchain required):

```sh
cargo install --git https://github.com/sanchpet/dispositif
```

Once a release is published, builds for macOS (`aarch64-apple-darwin`, `x86_64-apple-darwin`) are attached to GitHub releases as `dispositif-<target>.tar.gz`, which mise can install:

```sh
mise use -g github:sanchpet/dispositif
```

## Usage

```sh
dispositif check --config /path/to/config.toml         # validate, print rules and tiers
dispositif watch --config /path/to/config.toml --once  # print admitted messages as JSON lines, run nothing
dispositif run   --config /path/to/config.toml         # poll and answer, forever
```

The config path can also come from `DISPOSITIF_CONFIG`.

`check` exits non-zero and lists every problem if the config cannot be parsed, a rule names a tier that does not exist, a peer or sender id is malformed, a restricted tier has no tools or a tool that is not read-only, an unrestricted tier sets `tools`, a `permission_mode` is unknown, or `fallback_reply` is empty. A tier `cwd` that does not exist on the machine is reported as a warning only.

`watch` prints one line per admitted message:

```json
{"event":"message","trust":"full","rule":"owner-dm","peer":"1000002","chat":"Owner","id":42,"from":"Owner","fromId":1000002,"replyTo":null,"type":"text","text":"hello"}
```

When a poll fails, `watch` prints `{"event":"error","error":"..."}` once and prints `{"event":"recovered"}` once polling works again. With `--once`, a failed cycle exits with status 1.

`run` logs one timestamped line to stderr per event, finished run (with cost and turns), and failure.

State lives in `$DISPOSITIF_STATE_DIR`, or `~/.local/state/dispositif` if that is unset. `state.json` holds the last processed message id and known agent message ids per chat. `sessions.json` holds the Claude session per chat and tier. Deleting both is safe, because old history is not replayed. `watch` and `run` share this state, so run only one of them against a given account.

## Configuration

[`examples/config.toml`](examples/config.toml) is a commented two-tier config with placeholder ids.

Top level:

| Key | Default | Meaning |
|---|---|---|
| `mcp_url` | `http://127.0.0.1:8788` | mcp-tg streamable HTTP endpoint; plain `http://` only |
| `interval_secs` | `10` | pause between poll cycles |
| `agent_id` | required | the agent account's Telegram user id; its own messages never match |
| `agent_username` | required | without `@`; `@<username>` anywhere in a message (case-insensitive, not followed by a letter, digit or `_`) is a mention |
| `claude_bin` | `claude` | path, or name looked up on `PATH` |
| `claude_env` | `{}` | extra environment for runs; a leading `~` in values is expanded |
| `session_ttl_secs` | `86400` | how long a chat's Claude session is resumed |
| `run_timeout_secs` | `900` | a run is killed after this and answered with `fallback_reply`; a run's whole process group is killed when it ends either way |
| `history` | `15` | recent messages given to the run as context |
| `typing_interval_secs` | `4` | how often the typing indicator is re-sent while a run works; Telegram drops it after about five seconds |
| `preamble` | required | first part of every prompt |
| `fallback_reply` | required, non-empty | posted when a run fails |

`[[rule]]`, tried in order, first match wins:

| Key | Meaning |
|---|---|
| `name` | unique label, shown in logs and events |
| `peer` | dialog id as mcp-tg prints it (`1000002` user, `-1000003` group, `-100…` supergroup or channel), or `*` for any chat |
| `from` | sender user ids (positive) this rule admits |
| `trigger` | `any`: every message. `mention_or_reply`: only a mention of the agent or a reply to one of its messages. In a direct message, `mention_or_reply` counts as satisfied. |
| `trust` | name of the `[tier.<name>]` that handles the message |

`[tier.<name>]`:

| Key | Meaning |
|---|---|
| `cwd` | working directory of the run; `~` expanded |
| `permission_mode` | passed as `--permission-mode` when set; one of `acceptEdits`, `auto`, `bypassPermissions`, `default`, `dontAsk`, `manual`, `plan` |
| `restricted` | `true` adds `--restricted --strict-mcp-config --tools <tools> --allowedTools <tools>`: `--tools` makes a tool available, `--allowedTools` grants it, and a headless run has no one to ask |
| `tools` | required when restricted, refused otherwise; comma-separated, from `Read`, `Grep`, `Glob`, `WebFetch`, `WebSearch` |
| `git_pull` | `true` runs `git pull --ff-only` in `cwd` before each run, so a tier without a shell reads current code; a failure is logged and the run goes on |
| `instructions` | added to the prompt after the preamble |

The prompt of each run is the preamble, then the tier instructions, then `Chat: <title> (peer <peer>). Recent messages:` followed by one `[id] name (reply to N): text` line per message, then `Answer this message [id] from <name>:` followed by the text. Blank lines separate the parts.

## Running under launchd

`~/Library/LaunchAgents/com.example.dispositif.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.example.dispositif</string>
  <key>ProgramArguments</key>
  <array>
    <string>/path/to/dispositif</string>
    <string>run</string>
    <string>--config</string>
    <string>/path/to/config.toml</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardErrorPath</key>
  <string>/path/to/logs/dispositif.log</string>
</dict>
</plist>
```

```sh
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.example.dispositif.plist
launchctl print gui/$(id -u)/com.example.dispositif | grep state   # state = running
tail -f /path/to/logs/dispositif.log                               # "runner started"
```

launchd starts jobs with a minimal `PATH`, so set `claude_bin` to an absolute path. Also give any tool the Claude profile starts (MCP servers, hooks) an absolute path, or add an `EnvironmentVariables` dictionary with `PATH`. If `claude` is not found, each run logs `run failed peer=<peer> msg=<id>: starting claude: No such file or directory (os error 2)` and the chat receives `fallback_reply`.

To stop: `launchctl bootout gui/$(id -u)/com.example.dispositif`.

## Relation to mcp-tg

[mcp-tg](https://github.com/lexfrei/mcp-tg) is an MCP server for the Telegram client API (a user account, not a bot). dispositif is only a client of it. It uses `tg_dialogs_list`, `tg_messages_list`, `tg_messages_get`, `tg_messages_mark_read`, `tg_typing_send` and `tg_messages_send`, with MCP protocol version `2025-11-25`, and accepts both plain JSON and SSE responses. Point `mcp_url` at the mcp-tg instance logged in as the agent's account, not your own: everything dispositif marks read or sends, it does as that account.

dispositif never keeps an MCP session open between cycles. mcp-tg sends keepalive pings on an SSE stream this client does not open, and closes sessions whose pings cannot be delivered.

## License

MIT
