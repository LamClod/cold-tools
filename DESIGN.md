# cold-tools Design Document

LAMCLOD tool protocol framework and built-in toolset.

## Architecture Overview

```
cold-tools
├── Protocol Layer    ─ Tool trait / ToolResult / Permission / ToolError
├── Registry          ─ ToolRegistry (register / deregister / dispatch / definitions)
├── Schema            ─ Type-safe JSON Schema builder (object/string/integer/number/boolean/array/enum)
├── Context           ─ ToolContext (cwd / root / task_id / user / cancelled / env)
├── Security          ─ Path validation / traversal detection / device paths / sensitive paths / dangerous commands
├── Provider Traits   ─ UserInteraction / SearchProvider / BrowserProvider / MediaProvider / MessageProvider / HomeProvider / AgentRuntime
├── Tier 1: Core      ─ 8 tools (always compiled, pure Rust, zero external deps beyond regex/walkdir/glob)
├── Tier 2: Agent     ─ 8 tools (agent intelligence, pure Rust)
├── Tier 3: Web       ─ 3 tools (feature = "web")
├── Tier 4: Browser   ─ 12 tools (feature = "browser", CDP protocol)
├── Tier 5: Sandbox   ─ 2 tools (feature = "sandbox")
├── Tier 6: Media     ─ 5 tools (feature = "media")
├── Tier 7: Kanban    ─ 9 tools (feature = "kanban")
├── Tier 8: Comms     ─ 8 tools (feature = "comms")
├── Tier 9: IoT       ─ 4 tools (feature = "iot")
├── Tier 10: Extra    ─ 7 tools (feature = "extra")
├── Skill System      ─ Markdown-based prompt skills (load / index / inject)
└── MCP Bridge        ─ External tool integration via MCP protocol (stdio / SSE transports)
```

**Total: 66 tools + Skill System + MCP Bridge**

## Tool Lifecycle

```
Define → Register → Discover → Dispatch → Permission Check → Execute → Guardrails → Truncate → Result
```

1. **Define** — implement `Tool` trait (name, description, schema, execute)
2. **Register** — add to `ToolRegistry` via `register()`
3. **Discover** — `get_definitions()` exports OpenAI-format function schemas for LLM consumption
4. **Dispatch** — `dispatch(name, args, ctx)` routes to the correct tool
5. **Permission** — check `tool.permission()`: Auto (skip), Ask (user.confirm), Confirm (explicit confirmation)
6. **Execute** — `tool.execute(args, ctx)` runs the async implementation
7. **Guardrails** — loop detection (2 failures = warn, 5 = block, 8 = halt), dangerous command detection
8. **Truncate** — `ToolResult.truncate(max_output_bytes)` with safe char-boundary truncation
9. **Result** — `ToolResult::Text | Json | Error | Empty` returned to the agent

## Core Protocol Types

### Tool Trait

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn toolset(&self) -> &'static str;          // default: "default"
    fn parameters_schema(&self) -> Value;
    fn execute(&self, args: Value, ctx: &ToolContext) -> Pin<Box<dyn Future<Output = Result<ToolResult, ToolError>>>>;
    fn is_available(&self) -> bool;              // default: true
    fn is_read_only(&self) -> bool;              // default: false
    fn max_output_bytes(&self) -> usize;         // default: 50_000
    fn permission(&self) -> Permission;          // default: Auto
    fn timeout_secs(&self) -> u64;               // default: 120
}
```

### ToolResult

| Variant | Description |
|---------|-------------|
| `Text(String)` | Plain text output |
| `Json(Value)` | Structured JSON output |
| `Error { message, recoverable }` | Error returned to the model (not a Rust error) |
| `Empty` | No output (e.g., think tool) |

### Permission

| Level | Behavior |
|-------|----------|
| `Auto` | Always auto-approve, no user interaction |
| `Ask` | Ask user before execution via `UserInteraction::confirm()` |
| `Confirm` | Require explicit confirmation with full action description |

### ToolError

| Variant | Description |
|---------|-------------|
| `Execution(String)` | Tool execution failed |
| `NotFound(String)` | Tool not found in registry |
| `PermissionDenied(String)` | User denied permission |
| `Blocked(String)` | Blocked by guardrails |
| `Timeout { tool, timeout_secs }` | Execution timed out |
| `PathViolation(String)` | Path security violation |
| `Io(std::io::Error)` | IO error |
| `Json(serde_json::Error)` | JSON serialization error |

### ToolContext

| Field | Type | Description |
|-------|------|-------------|
| `cwd` | `PathBuf` | Current working directory |
| `root` | `PathBuf` | Security root — all paths must resolve within this |
| `task_id` | `String` | Task identifier |
| `user` | `Arc<dyn UserInteraction>` | User interaction handler |
| `cancelled` | `Arc<AtomicBool>` | Cooperative cancellation flag |
| `env` | `HashMap<String, String>` | Environment variables for subprocesses |

## Provider Traits

External services are injected via trait objects. Tools depend on traits, never concrete implementations.

| Trait | Methods | Used By |
|-------|---------|---------|
| `UserInteraction` | `ask(question) -> String`, `confirm(action) -> bool`, `notify(message)` | clarify, terminal (danger confirm) |
| `SearchProvider` | `search(query, max_results) -> Vec<SearchResult>` | web_search, x_search |
| `BrowserProvider` | `navigate()`, `snapshot()`, `click()`, `type_text()`, `scroll()`, `back()`, `press()`, `get_images()`, `screenshot()`, `execute_js()`, `cdp()`, `handle_dialog()` | All browser_* tools |
| `MediaProvider` | `generate_image()`, `generate_video()`, `text_to_speech()`, `analyze_image()`, `analyze_video()` | All media tools, browser_vision |
| `MessageProvider` | `send(channel, content, format)` | send_message, discord, feishu_* |
| `HomeProvider` | `list_entities()`, `get_state()`, `list_services()`, `call_service()` | All ha_* tools |
| `AgentRuntime` | `spawn(goal, context, max_turns) -> AgentResult` | delegate_task |

## Security Model

### Path Validation (`security::validate_path`)
1. Resolve path relative to `ctx.cwd` (if relative) or use as-is (if absolute)
2. Canonicalize the path (walk up to deepest existing ancestor for new files)
3. Verify canonical path starts with canonical `ctx.root`
4. Reject device paths (`/dev/`, `/proc/`, `/sys/`, `\\.\\`)
5. Reject sensitive paths (`.ssh`, `.gnupg`, `.aws`, `.kube`, `.docker`, `.env`, `.credentials`, etc.)

### Dangerous Command Detection (`security::detect_dangerous_command`)
- **Critical** (requires Confirm): `rm -rf /`, `rm -rf ~`, `dd if=`, `mkfs`, fork bombs
- **Warning** (user notified): `git push -f`, `git reset --hard`, `chmod 777`, `curl | sh`, `shutdown`, `reboot`

### Output Guardrails
- Max output: 50 KB / 2000 lines / 2000 chars per line (configurable via `CoreToolConfig`)
- Truncation uses safe char-boundary detection for multibyte UTF-8

## Feature Flags

| Feature | Tier | Tools | External Dependencies |
|---------|------|-------|----------------------|
| *(always)* | 1 Core | 8 | regex, walkdir, glob |
| *(always)* | 2 Agent | 8 | none (pure Rust + local files) |
| `web` | 3 Web | 3 | reqwest, scraper |
| `browser` | 4 Browser | 12 | tokio-tungstenite (CDP WebSocket) |
| `sandbox` | 5 Sandbox | 2 | none (subprocess isolation) |
| `media` | 6 Media | 5 | reqwest (provider HTTP calls) |
| `kanban` | 7 Kanban | 9 | none (local JSON files) |
| `comms` | 8 Comms | 8 | reqwest (Discord/Feishu APIs) |
| `iot` | 9 IoT | 4 | reqwest (Home Assistant REST API) |
| `extra` | 10 Extra | 7 | varies per tool |

---

## Tier 1: Core (8 tools) — always compiled

### 1. read_file
- **Toolset**: core
- **Description**: Read file contents with line numbers, offset, and limit support
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| path | string | yes | — | — | File path to read (relative to cwd) |
| offset | integer | no | 1 | min: 1 | Start line number (1-based) |
| limit | integer | no | — | min: 1 | Maximum number of lines to read |

**Execute logic**:
1. Extract `path` (required), `offset` (default 1, clamped to min 1), `limit` (optional)
2. Resolve path relative to `ctx.cwd`
3. Validate path within `ctx.root` via `security::validate_path`
4. Read entire file as raw bytes via `tokio::fs::read`
5. Binary detection: scan first 8 KB for null bytes (`\0`)
6. If binary: return `ToolResult::text("Binary file detected: {path} ({size} bytes)")` — not an error
7. Decode bytes as UTF-8 (lossy replacement for invalid sequences)
8. Split content into lines, compute `start = offset - 1` and `end = start + limit` (or all remaining)
9. Format each line as `"{line_num}\t{content}\n"` (1-based line numbers)
10. If output exceeds `max_read_bytes`: truncate via `ToolResult::truncate()` which appends `[truncated]`
11. Return formatted text

---

### 2. write_file
- **Toolset**: core
- **Description**: Create or overwrite a file with auto-created parent directories
- **Permission**: Ask | **Read-only**: No | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| path | string | yes | — | — | File path to write (relative to cwd) |
| content | string | yes | — | — | Content to write to the file |

**Execute logic**:
1. Extract `path` and `content` (both required)
2. Resolve path relative to `ctx.cwd`
3. Validate path within `ctx.root` via `security::validate_path`
4. Create parent directories recursively via `tokio::fs::create_dir_all`
5. Write content to file via `tokio::fs::write`
6. Return `"Wrote {byte_count} bytes to '{path}'"`

---

### 3. edit_file
- **Toolset**: core
- **Description**: Edit a file by replacing text occurrences (old_string to new_string with uniqueness check)
- **Permission**: Ask | **Read-only**: No | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| path | string | yes | — | — | File path to edit |
| old_string | string | yes | — | — | Text to find and replace |
| new_string | string | yes | — | — | Replacement text |
| replace_all | boolean | no | false | — | Replace all occurrences instead of requiring exactly one |

**Execute logic**:
1. Extract `path`, `old_string`, `new_string` (all required), `replace_all` (default false)
2. Resolve and validate path
3. Read file content as UTF-8 string via `tokio::fs::read_to_string`
4. Count occurrences of `old_string` in content
5. If count == 0: return error `"old_string not found in '{path}'"`
6. If count > 1 AND `replace_all` is false: return error `"old_string found {count} times — use replace_all=true or provide a more specific string"`
7. If `replace_all`: use `content.replace(old, new)`; else use `content.replacen(old, new, 1)`
8. Write modified content back to file
9. Return `"Replaced {count} occurrence(s) in '{path}'"`

---

### 4. search_files
- **Toolset**: core
- **Description**: Search file contents using regex patterns with glob filtering and context lines
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| pattern | string | yes | — | valid regex | Regex pattern to search for |
| path | string | no | "." | — | Directory to search in |
| glob_pattern | string | no | — | valid glob | File name glob filter (e.g., "*.rs") |
| max_results | integer | no | 50 | min: 1 | Maximum number of matches to return |
| context_lines | integer | no | 0 | min: 0 | Lines of context around each match |

**Execute logic**:
1. Extract `pattern` (required), compile as `regex::Regex`; return error on invalid regex
2. Parse optional `path` (default "."), `glob_pattern`, `max_results` (default 50), `context_lines` (default 0)
3. Resolve and validate search directory path
4. Compile optional glob pattern via `glob::Pattern::new`; return error on invalid glob
5. Walk directory tree via `walkdir::WalkDir` with `follow_links(false)`
6. Skip well-known noise directories: `.git`, `node_modules`, `target`, `__pycache__`, `.venv`, `venv`, `.idea`, `.vs`, `dist`, `build`
7. For each file entry:
   a. Check `ctx.is_cancelled()` for cooperative cancellation
   b. Apply glob filter against file name (not full path)
   c. Read file as bytes; skip files that fail to read
   d. Binary detection: skip files with null bytes in first 8 KB
   e. Parse as strict UTF-8; skip files that fail
   f. For each line matching the regex:
      - If `context_lines > 0`: output block with `>` marker on match line, surrounding context
      - If `context_lines == 0`: output `{rel_path}:{line_num}:{line_content}`
      - Stop when `max_results` reached
8. If no matches: return `"No matches found for pattern '{pattern}' in '{path}'"`
9. Return `"Found {count} match(es):\n{results}"`

---

### 5. list_dir
- **Toolset**: core
- **Description**: List directory contents with file types and sizes, supporting recursion depth control
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| path | string | no | "." | — | Directory path to list |
| depth | integer | no | 1 | min: 1 | Recursion depth (1 = no recursion) |
| show_hidden | boolean | no | false | — | Show hidden files (starting with '.') |

**Execute logic**:
1. Extract `path` (default "."), `depth` (default 1), `show_hidden` (default false)
2. Resolve and validate directory path
3. If path is not a directory: return error `"'{path}' is not a directory"`
4. Walk directory via `walkdir::WalkDir` with `min_depth(1)`, `max_depth(depth)`, `follow_links(false)`, `sort_by_file_name()`
5. For each entry:
   a. Compute relative path from search directory
   b. Skip hidden files (names starting with '.') unless `show_hidden` is true
   c. Determine type: `dir`, `symlink`, or `file`
   d. For files: read metadata to get size in bytes
6. Sort entries: directories first, then files, alphabetically within each group
7. If empty: return `"Empty directory: '{path}'"`
8. Format as aligned table with columns: name, kind, size (formatted as B/KB/MB/GB)
9. Return formatted output

---

### 6. terminal
- **Toolset**: core
- **Description**: Execute a shell command with timeout, output limits, and dangerous command detection
- **Permission**: Ask | **Read-only**: No | **Timeout**: 120s (configurable)
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| command | string | yes | — | — | Shell command to execute |
| timeout | integer | no | — | min: 1 | Override default timeout in seconds |

**Execute logic**:
1. Extract `command` (required) and optional `timeout` override
2. Run `security::detect_dangerous_command(command)`:
   a. If `Critical`: call `ctx.user.confirm("CRITICAL: {msg}\nProceed anyway?")`; if denied, return `PermissionDenied` error
   b. If `Warning`: call `ctx.user.notify("Warning: {msg}")` (non-blocking notification)
3. Determine effective timeout: `timeout` param override or `config.terminal_timeout`
4. Spawn subprocess:
   - Unix: `sh -c "{command}"` with cwd and env from context
   - Windows: `cmd /C "{command}"` with cwd and env from context
   - Capture stdout and stderr via piped handles
5. Wait for process completion with `tokio::time::timeout`
6. On success: read stdout and stderr buffers (lossy UTF-8), format as:
   ```
   Exit code: {code}
   --- stdout ---
   {stdout}
   --- stderr ---
   {stderr}
   ```
7. On process error: return `ToolError::Execution`
8. On timeout: kill the process via `child.kill()`, return `ToolError::Timeout`

---

### 7. process
- **Toolset**: core
- **Description**: Manage background processes (start, list, stop by PID)
- **Permission**: Ask | **Read-only**: No | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "start" \| "list" \| "stop" | Action to perform |
| command | string | no | — | required for "start" | Shell command to run in background |
| pid | integer | no | — | required for "stop" | Process ID to stop |

**Execute logic**:
1. Extract `action` (required); dispatch to sub-handler

**action = "start"**:
2. Extract `command` (required for start)
3. Spawn subprocess (same platform logic as terminal: `sh -c` / `cmd /C`) with piped stdout/stderr
4. Get PID from child handle; error if process exited immediately (no PID)
5. Store `ProcessInfo { command, child }` in `Arc<Mutex<HashMap<u32, ProcessInfo>>>`
6. Return `"Started background process (PID: {pid}): {command}"`

**action = "list"**:
7. Lock process registry
8. For each managed process: call `try_wait()` to check status
9. Format as tab-separated table: `PID\tStatus\tCommand`
10. Clean up finished processes from registry
11. If empty: return `"No managed processes"`

**action = "stop"**:
12. Extract `pid` (required for stop)
13. Remove process from registry; error if PID not found
14. Kill process via `child.kill().await`
15. Return `"Stopped process (PID: {pid}): {command}"`

---

### 8. think
- **Toolset**: core
- **Description**: Reasoning scratchpad for step-by-step thinking (no output to user)
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none
- **Status**: ✅ Implemented

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| thought | string | yes | — | — | The reasoning content |

**Execute logic**:
1. Accept `thought` parameter (required but content is not used)
2. Return `ToolResult::Empty` immediately
3. The thought lives in the conversation context (visible to the model) but produces no output to the user
4. `max_output_bytes` is 0 (no output ever)

---

## Tier 2: Agent Intelligence (8 tools) — always compiled

### 9. clarify
- **Toolset**: agent
- **Description**: Ask the user a question and wait for their response
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 300s
- **Feature**: always compiled
- **Provider**: `UserInteraction` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| question | string | yes | — | — | The question to ask the user |
| options | array of strings | no | — | — | Multiple choice options (if provided, user picks one) |

**Execute logic**:
1. Extract `question` (required) and `options` (optional)
2. If `options` is provided and non-empty:
   a. Format question with numbered options: `"{question}\n1. {opt1}\n2. {opt2}\n..."`
   b. Call `ctx.user.ask(formatted_question)`
3. If `options` is not provided:
   a. Call `ctx.user.ask(question)` for free-form response
4. Return the user's answer as `ToolResult::text(answer)`
5. If answer is empty (e.g., AutoApprove no-op): return `ToolResult::text("(no response)")`

---

### 10. memory
- **Toolset**: agent
- **Description**: Persistent key-value memory store (read/write/search/delete/list)
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none (local filesystem)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "read" \| "write" \| "search" \| "delete" \| "list" | Operation to perform |
| key | string | no | — | required for read/write/delete | Memory key |
| value | string | no | — | required for write | Value to store |
| query | string | no | — | required for search | Fuzzy search query |

**Execute logic**:

**Storage**: `{ctx.root}/.cold/memory/memory.json` — JSON object `{ "key": { "value": "...", "updated_at": "..." } }`

**action = "write"**:
1. Extract `key` and `value` (both required)
2. Load or create memory.json
3. Insert/update entry with current timestamp
4. Write back to file
5. Return `"Stored key '{key}'"`

**action = "read"**:
6. Extract `key` (required)
7. Load memory.json
8. Look up key; return error if not found
9. Return value

**action = "search"**:
10. Extract `query` (required)
11. Load memory.json
12. Fuzzy match `query` against all keys and values (case-insensitive substring match)
13. Return matching entries formatted as `"{key}: {value_preview}"`

**action = "delete"**:
14. Extract `key` (required)
15. Load memory.json
16. Remove key; error if not found
17. Write back, return `"Deleted key '{key}'"`

**action = "list"**:
18. Load memory.json
19. Return all keys with truncated value previews (first 100 chars)
20. If empty: return `"No memories stored"`

---

### 11. todo
- **Toolset**: agent
- **Description**: Task management with status tracking (add/list/update/remove/clear)
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none (local filesystem)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "add" \| "list" \| "update" \| "remove" \| "clear" | Action to perform |
| task | string | no | — | required for "add" | Task description |
| id | integer | no | — | required for update/remove | Task ID |
| status | enum | no | — | "pending" \| "in_progress" \| "done" \| "blocked" | New status (for update) |

**Execute logic**:

**Storage**: `{ctx.root}/.cold/todo.json` — JSON array of `{ "id": N, "description": "...", "status": "pending", "created_at": "...", "updated_at": "..." }`

**action = "add"**:
1. Extract `task` (required)
2. Load or create todo.json
3. Assign next sequential ID (max existing + 1, or 1 if empty)
4. Create entry with status "pending" and current timestamps
5. Write back, return `"Added task #{id}: {task}"`

**action = "list"**:
6. Load todo.json; return `"No tasks"` if empty
7. Format tasks as table: `ID | Status | Description`
8. Group by status: in_progress first, then pending, blocked, done

**action = "update"**:
9. Extract `id` (required) and `status` (optional)
10. Find task by ID; error if not found
11. Update status and `updated_at` timestamp
12. Write back, return `"Updated task #{id} → {status}"`

**action = "remove"**:
13. Extract `id` (required)
14. Remove task by ID; error if not found
15. Write back, return `"Removed task #{id}"`

**action = "clear"**:
16. Delete all tasks (or remove only "done" tasks)
17. Write back, return `"Cleared {count} task(s)"`

---

### 12. delegate_task
- **Toolset**: agent
- **Description**: Spawn a sub-agent to execute a goal independently
- **Permission**: Ask | **Read-only**: No | **Timeout**: 600s
- **Feature**: always compiled
- **Provider**: `AgentRuntime` trait (defined in cold-agent crate)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| goal | string | yes | — | — | What the sub-agent should accomplish |
| context | string | no | — | — | Additional context or constraints for the sub-agent |
| max_turns | integer | no | 30 | min: 1, max: 100 | Maximum conversation turns before forcing completion |

**Execute logic**:
1. Extract `goal` (required), `context` (optional), `max_turns` (default 30)
2. Check if `AgentRuntime` provider is available; if not, return error `"delegate_task requires an AgentRuntime provider (cold-agent crate)"`
3. Build sub-agent context: inherit `ctx.root`, `ctx.cwd`, `ctx.env`; generate new `task_id`
4. Call `runtime.spawn(goal, context, max_turns)` — blocks until sub-agent completes or hits max_turns
5. Periodically check `ctx.is_cancelled()` for cooperative cancellation
6. On completion: return sub-agent's final output as `ToolResult::text`
7. On max_turns exceeded: return partial result with note `"[sub-agent reached max_turns limit]"`
8. On error: return `ToolResult::error` with sub-agent error message

---

### 13. session_search
- **Toolset**: agent
- **Description**: Search conversation history across saved sessions
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none (local filesystem)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| query | string | yes | — | — | Search query (fuzzy matched against message content) |
| max_results | integer | no | 10 | min: 1, max: 100 | Maximum number of results to return |

**Execute logic**:

**Storage**: `{ctx.root}/.cold/sessions/` — one JSON file per session, each containing an array of messages

1. Extract `query` (required) and `max_results` (default 10)
2. Scan `{ctx.root}/.cold/sessions/` directory for `*.json` files
3. If directory does not exist or is empty: return `"No sessions found"`
4. For each session file (sorted by modification time, newest first):
   a. Parse JSON as array of messages
   b. For each message: fuzzy match `query` against content (case-insensitive substring)
   c. Collect matches with session ID, message index, and content preview (first 200 chars)
5. Sort results by relevance (exact match > prefix match > substring match)
6. Truncate to `max_results`
7. Format as `"{session_id} [{role}] {preview}..."` per match
8. Return formatted results

---

### 14. skill_manage
- **Toolset**: agent
- **Description**: Install, enable, disable, or uninstall skills
- **Permission**: Ask | **Read-only**: No | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none (Skill System filesystem)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "install" \| "enable" \| "disable" \| "uninstall" | Action to perform |
| name | string | yes | — | — | Skill name (e.g., "code-review") |
| source | string | no | — | required for "install" | URL or local path to skill package |

**Execute logic**:

**Storage**: `{ctx.root}/.cold/skills/{name}/SKILL.md` — each skill is a directory with a SKILL.md file

**action = "install"**:
1. Extract `name` and `source` (required for install)
2. If source is a URL: download via HTTP GET
3. If source is a local path: copy files
4. Create `{ctx.root}/.cold/skills/{name}/` directory
5. Write SKILL.md (and any additional files from source)
6. Return `"Installed skill '{name}'"`

**action = "enable"**:
7. Extract `name` (required)
8. Check skill directory exists; error if not
9. Remove `.disabled` marker file if present
10. Return `"Enabled skill '{name}'"`

**action = "disable"**:
11. Extract `name` (required)
12. Check skill directory exists; error if not
13. Create `.disabled` marker file in skill directory
14. Return `"Disabled skill '{name}'"`

**action = "uninstall"**:
15. Extract `name` (required)
16. Check skill directory exists; error if not
17. Remove entire skill directory recursively
18. Return `"Uninstalled skill '{name}'"`

---

### 15. skills_list
- **Toolset**: agent
- **Description**: List available skills with optional filtering
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none (Skill System filesystem)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| filter | string | no | — | — | Filter skills by name substring |
| show_disabled | boolean | no | false | — | Include disabled skills in the listing |

**Execute logic**:
1. Extract `filter` (optional) and `show_disabled` (default false)
2. Scan `{ctx.root}/.cold/skills/` directory for subdirectories
3. If directory does not exist: return `"No skills installed"`
4. For each skill directory:
   a. Check for `.disabled` marker file; skip if disabled and `show_disabled` is false
   b. Read `SKILL.md` frontmatter (YAML between `---` delimiters) for: name, description, version, triggers
   c. If `filter` is provided: skip skills whose name does not contain the filter substring
5. Format as table: `Name | Version | Status | Description`
6. Sort alphabetically by name
7. Return formatted listing

---

### 16. skill_view
- **Toolset**: agent
- **Description**: View full details of a specific skill
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: always compiled
- **Provider**: none (Skill System filesystem)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| name | string | yes | — | — | Skill name to view |

**Execute logic**:
1. Extract `name` (required)
2. Construct path: `{ctx.root}/.cold/skills/{name}/SKILL.md`
3. Check if file exists; error `"Skill '{name}' not found"` if not
4. Read SKILL.md content as UTF-8
5. Return full content as `ToolResult::text`

---

## Tier 3: Web (3 tools) — feature = "web"

### 17. web_search
- **Toolset**: web
- **Description**: Search the internet using a search engine provider
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `web`
- **Provider**: `SearchProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| query | string | yes | — | — | Search query |
| max_results | integer | no | 10 | min: 1, max: 50 | Maximum number of results |

**Execute logic**:
1. Extract `query` (required) and `max_results` (default 10)
2. Check if `SearchProvider` is available; error if not configured
3. Call `provider.search(query, max_results)`
4. For each result: format as `"{title}\n{url}\n{snippet}\n"`
5. If no results: return `"No results found for '{query}'"`
6. Return formatted results

---

### 18. web_extract
- **Toolset**: web
- **Description**: Fetch a URL and extract readable text content
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 60s
- **Feature**: `web`
- **Provider**: none (uses reqwest + scraper directly)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| url | string | yes | — | valid URL | URL to fetch |
| selector | string | no | — | valid CSS selector | CSS selector to extract specific elements |
| max_length | integer | no | 50000 | min: 1000 | Maximum characters to return |

**Execute logic**:
1. Extract `url` (required), `selector` (optional), `max_length` (default 50000)
2. Validate URL format (must start with http:// or https://)
3. HTTP GET the URL via reqwest with reasonable timeout (30s), follow redirects (max 5)
4. Set User-Agent header to avoid bot blocking
5. If response is not 2xx: return error `"HTTP {status}: {reason}"`
6. Get response body as text
7. Parse HTML using scraper crate
8. If `selector` is provided:
   a. Parse CSS selector; error on invalid selector
   b. Select matching elements
   c. Extract text content from each matched element
9. If no `selector`: extract all visible text (strip `<script>`, `<style>`, `<nav>`, `<header>`, `<footer>` tags)
10. Collapse whitespace, trim
11. Truncate to `max_length` characters
12. Return extracted text

---

### 19. x_search
- **Toolset**: web
- **Description**: Search Twitter/X for posts matching a query
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `web`
- **Provider**: `SearchProvider` trait (same trait, X-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| query | string | yes | — | — | Search query for Twitter/X |
| max_results | integer | no | 10 | min: 1, max: 50 | Maximum number of results |

**Execute logic**:
1. Extract `query` (required) and `max_results` (default 10)
2. Check if X-specific `SearchProvider` is available; error if not configured
3. Call `provider.search(query, max_results)`
4. For each result: format as `"@{author} ({date}):\n{content}\n{url}\n"`
5. If no results: return `"No results found for '{query}'"`
6. Return formatted results

---

## Tier 4: Browser (12 tools) — feature = "browser"

All browser tools belong to the `browser` toolset and require the `BrowserProvider` trait. The provider implementation handles Chrome DevTools Protocol (CDP) over WebSocket.

### 20. browser_navigate
- **Toolset**: browser
- **Description**: Navigate the browser to a URL
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| url | string | yes | — | valid URL | URL to navigate to |

**Execute logic**:
1. Extract `url` (required)
2. Validate URL format
3. Call `provider.navigate(url)` — sends CDP `Page.navigate` command
4. Wait for page load event (with timeout)
5. Return `"Navigated to {url}"` with page title if available

---

### 21. browser_snapshot
- **Toolset**: browser
- **Description**: Capture current page state as HTML, text, or screenshot
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| format | enum | no | "text" | "html" \| "text" \| "screenshot" | Output format |

**Execute logic**:
1. Extract `format` (default "text")
2. If "html": call `provider.snapshot_html()` — returns raw DOM HTML
3. If "text": call `provider.snapshot_text()` — returns visible text content with element annotations
4. If "screenshot": call `provider.screenshot()` — returns base64-encoded PNG
5. Truncate text output to `max_output_bytes`
6. Return result (text for html/text, base64 string for screenshot)

---

### 22. browser_click
- **Toolset**: browser
- **Description**: Click an element on the page by CSS selector
- **Permission**: Ask | **Read-only**: No | **Timeout**: 10s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| selector | string | yes | — | valid CSS selector | CSS selector for the element to click |

**Execute logic**:
1. Extract `selector` (required)
2. Call `provider.click(selector)` — CDP: query selector, get element center coordinates, dispatch mouse events
3. If element not found: return error `"Element not found: '{selector}'"`
4. Wait briefly for any navigation or DOM update
5. Return `"Clicked '{selector}'"`

---

### 23. browser_type
- **Toolset**: browser
- **Description**: Type text into a focused or selected element
- **Permission**: Ask | **Read-only**: No | **Timeout**: 10s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| selector | string | yes | — | valid CSS selector | CSS selector for the input element |
| text | string | yes | — | — | Text to type |
| clear | boolean | no | false | — | Clear existing content before typing |

**Execute logic**:
1. Extract `selector` (required), `text` (required), `clear` (default false)
2. Call `provider.click(selector)` to focus the element
3. If element not found: return error `"Element not found: '{selector}'"`
4. If `clear`: select all text (Ctrl+A / Cmd+A) then delete
5. Call `provider.type_text(text)` — dispatches key events for each character
6. Return `"Typed {len} characters into '{selector}'"`

---

### 24. browser_scroll
- **Toolset**: browser
- **Description**: Scroll the page in a given direction
- **Permission**: Auto | **Read-only**: No | **Timeout**: 5s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| direction | enum | yes | — | "up" \| "down" \| "left" \| "right" | Scroll direction |
| amount | integer | no | 300 | min: 1 | Scroll amount in pixels |

**Execute logic**:
1. Extract `direction` (required) and `amount` (default 300)
2. Compute deltaX/deltaY based on direction:
   - up: deltaY = -amount
   - down: deltaY = amount
   - left: deltaX = -amount
   - right: deltaX = amount
3. Call `provider.scroll(deltaX, deltaY)` — dispatches wheel event via CDP
4. Return `"Scrolled {direction} {amount}px"`

---

### 25. browser_back
- **Toolset**: browser
- **Description**: Navigate back in browser history
- **Permission**: Auto | **Read-only**: No | **Timeout**: 10s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| *(none)* | — | — | — | — | — |

**Execute logic**:
1. Call `provider.back()` — CDP: `Page.navigateHistory` with delta=-1
2. Wait for page load event
3. Return `"Navigated back"` with new URL if available

---

### 26. browser_press
- **Toolset**: browser
- **Description**: Press a keyboard key
- **Permission**: Auto | **Read-only**: No | **Timeout**: 5s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| key | string | yes | — | — | Key name (e.g., "Enter", "Tab", "Escape", "ArrowDown") |

**Execute logic**:
1. Extract `key` (required)
2. Map key name to CDP key code/descriptor
3. Call `provider.press(key)` — dispatches `Input.dispatchKeyEvent` (keyDown + keyUp)
4. Return `"Pressed key '{key}'"`

---

### 27. browser_get_images
- **Toolset**: browser
- **Description**: Get all images on the current page with URLs and alt text
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 10s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| max_images | integer | no | 20 | min: 1 | Maximum number of images to return |

**Execute logic**:
1. Extract `max_images` (default 20)
2. Call `provider.get_images(max_images)` — CDP: execute JS to query all `<img>` elements
3. For each image: extract `src` (resolve relative URLs), `alt` text, `width`, `height`
4. Filter out data URIs and tracking pixels (1x1 images)
5. Format as `"{index}. {alt_text}\n   {url}\n   {width}x{height}\n"`
6. Return formatted list

---

### 28. browser_vision
- **Toolset**: browser
- **Description**: Analyze the current page visually by taking a screenshot and sending it to a vision model
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 60s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait + `MediaProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| question | string | yes | — | — | Question about the page's visual content |

**Execute logic**:
1. Extract `question` (required)
2. Call `provider.screenshot()` — CDP: `Page.captureScreenshot` (full page or viewport)
3. Get screenshot as PNG bytes
4. Check if `MediaProvider` is available; error if not
5. Call `media_provider.analyze_image(screenshot_bytes, question)`
6. Return vision model's analysis as text

---

### 29. browser_console
- **Toolset**: browser
- **Description**: Execute JavaScript in the browser console
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| expression | string | yes | — | — | JavaScript expression to evaluate |

**Execute logic**:
1. Extract `expression` (required)
2. Call `provider.execute_js(expression)` — CDP: `Runtime.evaluate`
3. If execution throws: return error with exception details
4. Serialize return value to string representation
5. Truncate output to `max_output_bytes`
6. Return `"Result: {serialized_value}"`

---

### 30. browser_cdp
- **Toolset**: browser
- **Description**: Send a raw Chrome DevTools Protocol command
- **Permission**: Confirm | **Read-only**: No | **Timeout**: 30s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| method | string | yes | — | — | CDP method name (e.g., "Network.enable", "DOM.getDocument") |
| params | object | no | {} | — | CDP method parameters |

**Execute logic**:
1. Extract `method` (required) and `params` (default empty object)
2. Call `provider.cdp(method, params)` — send raw CDP message over WebSocket
3. Wait for response with timeout
4. Return response JSON as `ToolResult::Json`
5. On CDP error: return error with CDP error code and message

---

### 31. browser_dialog
- **Toolset**: browser
- **Description**: Handle browser dialogs (alert, confirm, prompt)
- **Permission**: Ask | **Read-only**: No | **Timeout**: 5s
- **Feature**: `browser`
- **Provider**: `BrowserProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "accept" \| "dismiss" | Whether to accept or dismiss the dialog |
| text | string | no | — | — | Text to enter (for prompt dialogs) |

**Execute logic**:
1. Extract `action` (required) and `text` (optional)
2. If `action` is "accept":
   a. Call `provider.handle_dialog(accept=true, text)` — CDP: `Page.handleJavaScriptDialog`
3. If `action` is "dismiss":
   a. Call `provider.handle_dialog(accept=false, None)`
4. Return `"Dialog {action}ed"`

---

## Tier 5: Sandbox (2 tools) — feature = "sandbox"

### 32. execute_code
- **Toolset**: sandbox
- **Description**: Run code in an isolated environment with restricted filesystem and network access
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `sandbox`
- **Provider**: none (subprocess isolation)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| language | enum | yes | — | "python" \| "javascript" \| "rust" \| "go" \| "bash" | Programming language to execute |
| code | string | yes | — | — | Source code to run |
| timeout | integer | no | 30 | min: 1, max: 300 | Execution timeout in seconds |

**Execute logic**:
1. Extract `language` (required), `code` (required), `timeout` (default 30)
2. Create temporary directory for execution (auto-cleaned on completion)
3. Write code to temp file with appropriate extension:
   - python: `script.py`
   - javascript: `script.js`
   - rust: `script.rs`
   - go: `script.go`
   - bash: `script.sh`
4. Build execution command:
   - python: `python3 script.py`
   - javascript: `node script.js`
   - rust: `rustc script.rs -o script && ./script`
   - go: `go run script.go`
   - bash: `bash script.sh`
5. Spawn subprocess with restrictions:
   - No network access (platform-specific: unshare on Linux, sandbox-exec on macOS)
   - Working directory limited to temp directory
   - No inherited environment variables (except PATH)
   - Stdout and stderr captured
6. Wait with timeout
7. On success: return combined stdout + stderr with exit code
8. On timeout: kill process, return error
9. Clean up temp directory

---

### 33. computer_use
- **Toolset**: sandbox
- **Description**: OS-level automation (screenshot, click, type, scroll at screen coordinates)
- **Permission**: Confirm | **Read-only**: No | **Timeout**: 30s
- **Feature**: `sandbox`
- **Provider**: OS-specific screen capture + input simulation
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "screenshot" \| "click" \| "type" \| "key" \| "scroll" \| "move" | Action to perform |
| x | integer | no | — | required for click/move | X screen coordinate |
| y | integer | no | — | required for click/move | Y screen coordinate |
| text | string | no | — | required for type | Text to type |
| key | string | no | — | required for key | Key combination (e.g., "ctrl+c") |

**Execute logic**:
1. Extract `action` (required) and action-specific parameters
2. **Always** requires Confirm permission (most dangerous tool)

**action = "screenshot"**:
3. Capture entire screen via platform API
4. Encode as base64 PNG
5. Return screenshot data

**action = "click"**:
6. Extract `x` and `y` (required)
7. Move mouse to coordinates, dispatch click event
8. Return `"Clicked at ({x}, {y})"`

**action = "type"**:
9. Extract `text` (required)
10. Simulate keyboard input for each character
11. Return `"Typed {len} characters"`

**action = "key"**:
12. Extract `key` (required, e.g., "ctrl+c", "alt+tab")
13. Parse key combination, dispatch key events
14. Return `"Pressed {key}"`

**action = "scroll"**:
15. Use current mouse position, dispatch scroll event
16. Extract implicit direction from params or default to down
17. Return `"Scrolled"`

**action = "move"**:
18. Extract `x` and `y` (required)
19. Move mouse to coordinates
20. Return `"Moved to ({x}, {y})"`

---

## Tier 6: Media (5 tools) — feature = "media"

All media tools belong to the `media` toolset and require the `MediaProvider` trait.

### 34. image_generate
- **Toolset**: media
- **Description**: Generate an image from a text prompt
- **Permission**: Ask | **Read-only**: No | **Timeout**: 120s
- **Feature**: `media`
- **Provider**: `MediaProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| prompt | string | yes | — | — | Image generation prompt |
| size | string | no | "1024x1024" | format: "WxH" | Output image dimensions |
| style | string | no | — | — | Style modifier (e.g., "photorealistic", "anime", "oil-painting") |

**Execute logic**:
1. Extract `prompt` (required), `size` (default "1024x1024"), `style` (optional)
2. If `style` is provided: prepend style to prompt or pass as separate parameter
3. Call `provider.generate_image(prompt, size, style)`
4. Receive image bytes (PNG/JPEG)
5. Save to temp file in `{ctx.root}/.cold/media/` with timestamp-based filename
6. Return file path and image metadata (size, format, byte count)

---

### 35. video_generate
- **Toolset**: media
- **Description**: Generate a short video from a text prompt
- **Permission**: Ask | **Read-only**: No | **Timeout**: 300s
- **Feature**: `media`
- **Provider**: `MediaProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| prompt | string | yes | — | — | Video generation prompt |
| duration | integer | no | 5 | min: 1, max: 60 | Duration in seconds |
| size | string | no | — | format: "WxH" | Output video dimensions |

**Execute logic**:
1. Extract `prompt` (required), `duration` (default 5), `size` (optional)
2. Call `provider.generate_video(prompt, duration, size)`
3. This may be a long-running operation; periodically check `ctx.is_cancelled()`
4. Receive video bytes (MP4)
5. Save to `{ctx.root}/.cold/media/` with timestamp-based filename
6. Return file path and metadata (duration, format, byte count)

---

### 36. text_to_speech
- **Toolset**: media
- **Description**: Convert text to audio speech
- **Permission**: Ask | **Read-only**: No | **Timeout**: 60s
- **Feature**: `media`
- **Provider**: `MediaProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| text | string | yes | — | — | Text to convert to speech |
| voice | string | no | "default" | — | Voice identifier |
| format | string | no | "mp3" | "mp3" \| "wav" \| "ogg" | Output audio format |

**Execute logic**:
1. Extract `text` (required), `voice` (default "default"), `format` (default "mp3")
2. Call `provider.text_to_speech(text, voice, format)`
3. Receive audio bytes
4. Save to `{ctx.root}/.cold/media/` with timestamp-based filename and correct extension
5. Return file path and metadata (duration estimate, format, byte count)

---

### 37. vision_analyze
- **Toolset**: media
- **Description**: Analyze an image file and answer questions about it
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 60s
- **Feature**: `media`
- **Provider**: `MediaProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| image_path | string | yes | — | — | Path to the image file |
| question | string | yes | — | — | Question about the image |

**Execute logic**:
1. Extract `image_path` (required) and `question` (required)
2. Resolve and validate path within `ctx.root`
3. Read image file as bytes
4. Detect image format from magic bytes (PNG/JPEG/GIF/WEBP)
5. If not a recognized image format: return error `"Not a recognized image format"`
6. Encode image as base64
7. Call `provider.analyze_image(image_bytes, question)`
8. Return analysis text

---

### 38. video_analyze
- **Toolset**: media
- **Description**: Analyze a video file and answer questions about it
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: `media`
- **Provider**: `MediaProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| video_path | string | yes | — | — | Path to the video file |
| question | string | yes | — | — | Question about the video |

**Execute logic**:
1. Extract `video_path` (required) and `question` (required)
2. Resolve and validate path within `ctx.root`
3. Read video file (or stream it to provider)
4. Call `provider.analyze_video(video_path, question)` — provider may extract key frames
5. Return analysis text

---

## Tier 7: Kanban (9 tools) — feature = "kanban"

All kanban tools belong to the `kanban` toolset and operate on a local JSON file.

**Storage**: `{ctx.root}/.cold/kanban/kanban.json`

**Data model**:
```json
{
  "boards": {
    "default": {
      "tasks": [
        {
          "id": "TASK-001",
          "title": "...",
          "description": "...",
          "status": "todo",
          "assignee": null,
          "priority": "medium",
          "comments": [],
          "links": [],
          "created_at": "...",
          "updated_at": "..."
        }
      ]
    }
  }
}
```

### 39. kanban_show
- **Toolset**: kanban
- **Description**: Display the full kanban board as a visual text representation
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| board | string | no | "default" | — | Board name |

**Execute logic**:
1. Extract `board` (default "default")
2. Load kanban.json; return empty board visualization if file does not exist
3. Group tasks by status columns: TODO | IN_PROGRESS | TESTING | DONE | BLOCKED
4. Format as ASCII columns with task IDs, titles, and priority indicators
5. Include task counts per column and overall summary
6. Return formatted board

---

### 40. kanban_list
- **Toolset**: kanban
- **Description**: List kanban tasks with optional filters
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| status | enum | no | — | "todo" \| "in_progress" \| "testing" \| "done" \| "blocked" | Filter by status |
| assignee | string | no | — | — | Filter by assignee |

**Execute logic**:
1. Extract optional `status` and `assignee` filters
2. Load kanban.json
3. Filter tasks by status and/or assignee
4. Format as table: `ID | Status | Priority | Assignee | Title`
5. If no matching tasks: return `"No tasks found"`
6. Return formatted list

---

### 41. kanban_create
- **Toolset**: kanban
- **Description**: Create a new kanban task
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| title | string | yes | — | — | Task title |
| description | string | no | — | — | Detailed task description |
| assignee | string | no | — | — | Assigned person or agent |
| priority | enum | no | "medium" | "low" \| "medium" \| "high" \| "critical" | Task priority |

**Execute logic**:
1. Extract `title` (required), `description`, `assignee`, `priority` (default "medium")
2. Load or create kanban.json
3. Generate task ID: `TASK-{next_sequential_number}` (zero-padded to 3 digits)
4. Create task entry with status "todo", current timestamps
5. Append to board's task list
6. Write back to file
7. Return `"Created {task_id}: {title}"`

---

### 42. kanban_complete
- **Toolset**: kanban
- **Description**: Mark a kanban task as done
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| task_id | string | yes | — | — | Task ID to complete |
| note | string | no | — | — | Completion note |

**Execute logic**:
1. Extract `task_id` (required) and `note` (optional)
2. Load kanban.json; find task by ID
3. If not found: return error `"Task '{task_id}' not found"`
4. Set status to "done", update `updated_at` timestamp
5. If `note` is provided: add as final comment with "completed" tag
6. Write back to file
7. Return `"Completed {task_id}: {title}"`

---

### 43. kanban_block
- **Toolset**: kanban
- **Description**: Block a kanban task with a reason
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| task_id | string | yes | — | — | Task ID to block |
| reason | string | yes | — | — | Reason for blocking |

**Execute logic**:
1. Extract `task_id` and `reason` (both required)
2. Load kanban.json; find task by ID
3. If not found: return error
4. Set status to "blocked", update timestamp
5. Add comment with reason tagged as "blocked"
6. Write back to file
7. Return `"Blocked {task_id}: {reason}"`

---

### 44. kanban_unblock
- **Toolset**: kanban
- **Description**: Unblock a previously blocked kanban task
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| task_id | string | yes | — | — | Task ID to unblock |

**Execute logic**:
1. Extract `task_id` (required)
2. Load kanban.json; find task by ID
3. If not found: return error
4. If status is not "blocked": return error `"Task '{task_id}' is not blocked"`
5. Set status to "todo" (or previous status if tracked), update timestamp
6. Add comment "unblocked"
7. Write back to file
8. Return `"Unblocked {task_id}"`

---

### 45. kanban_heartbeat
- **Toolset**: kanban
- **Description**: Update progress on an in-progress kanban task
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| task_id | string | yes | — | — | Task ID to update |
| progress | string | yes | — | — | Progress description (e.g., "50% complete", "parsing module done") |

**Execute logic**:
1. Extract `task_id` and `progress` (both required)
2. Load kanban.json; find task by ID
3. If not found: return error
4. If status is "todo": automatically transition to "in_progress"
5. Update timestamp
6. Add comment with progress text tagged as "heartbeat"
7. Write back to file
8. Return `"Heartbeat {task_id}: {progress}"`

---

### 46. kanban_comment
- **Toolset**: kanban
- **Description**: Add a comment to a kanban task
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| task_id | string | yes | — | — | Task ID to comment on |
| comment | string | yes | — | — | Comment text |

**Execute logic**:
1. Extract `task_id` and `comment` (both required)
2. Load kanban.json; find task by ID
3. If not found: return error
4. Append comment with current timestamp to task's comments array
5. Update task's `updated_at` timestamp
6. Write back to file
7. Return `"Added comment to {task_id}"`

---

### 47. kanban_link
- **Toolset**: kanban
- **Description**: Create a relationship link between two kanban tasks
- **Permission**: Auto | **Read-only**: No | **Timeout**: 120s
- **Feature**: `kanban`
- **Provider**: none
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| from_id | string | yes | — | — | Source task ID |
| to_id | string | yes | — | — | Target task ID |
| relation | enum | yes | — | "blocks" \| "depends_on" \| "related" | Type of relationship |

**Execute logic**:
1. Extract `from_id`, `to_id`, and `relation` (all required)
2. Load kanban.json; verify both tasks exist
3. If either not found: return error
4. Add link entry `{ "target": to_id, "relation": relation }` to source task's links array
5. Add reciprocal link to target task (blocks <-> depends_on, related <-> related)
6. Write back to file
7. Return `"Linked {from_id} --{relation}--> {to_id}"`

---

## Tier 8: Comms (8 tools) — feature = "comms"

### 48. send_message
- **Toolset**: comms
- **Description**: Send a message to a generic channel
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| channel | string | yes | — | — | Target channel identifier |
| content | string | yes | — | — | Message content |
| format | enum | no | "text" | "text" \| "markdown" | Message format |

**Execute logic**:
1. Extract `channel` (required), `content` (required), `format` (default "text")
2. Check if `MessageProvider` is available; error if not configured
3. Call `provider.send(channel, content, format)`
4. Return `"Message sent to '{channel}'"`

---

### 49. discord
- **Toolset**: comms
- **Description**: Send a message to a Discord channel
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait (Discord-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| channel_id | string | yes | — | — | Discord channel ID |
| content | string | yes | — | — | Message content |
| embed | object | no | — | — | Discord embed object (title, description, color, fields, etc.) |

**Execute logic**:
1. Extract `channel_id` (required), `content` (required), `embed` (optional)
2. Build Discord API payload:
   - If `embed` provided: include in message payload
   - Validate embed structure (title, description, fields)
3. Call Discord REST API: `POST /channels/{channel_id}/messages`
4. On success: return `"Message sent to Discord channel {channel_id}"`
5. On API error: return error with Discord error code and message

---

### 50. discord_admin
- **Toolset**: comms
- **Description**: Discord administrative operations (list channels, members, manage messages)
- **Permission**: Confirm | **Read-only**: No | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait (Discord-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "list_channels" \| "list_members" \| "create_channel" \| "delete_message" | Admin action |
| guild_id | string | no | — | required for list_channels/list_members/create_channel | Discord guild (server) ID |
| channel_id | string | no | — | required for delete_message | Channel ID |
| message_id | string | no | — | required for delete_message | Message ID to delete |
| name | string | no | — | required for create_channel | New channel name |

**Execute logic**:

**action = "list_channels"**:
1. Extract `guild_id` (required)
2. Call Discord API: `GET /guilds/{guild_id}/channels`
3. Format as list: `#{name} ({type}) - {id}`

**action = "list_members"**:
4. Extract `guild_id` (required)
5. Call Discord API: `GET /guilds/{guild_id}/members`
6. Format as list: `{username}#{discriminator} - {id}`

**action = "create_channel"**:
7. Extract `guild_id` and `name` (both required)
8. Call Discord API: `POST /guilds/{guild_id}/channels` with `{ "name": name }`
9. Return `"Created channel #{name} ({new_id})"`

**action = "delete_message"**:
10. Extract `channel_id` and `message_id` (both required)
11. Call Discord API: `DELETE /channels/{channel_id}/messages/{message_id}`
12. Return `"Deleted message {message_id}"`

---

### 51. feishu_doc_read
- **Toolset**: comms
- **Description**: Read a Feishu/Lark document
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait (Feishu-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| doc_id | string | yes | — | — | Feishu document ID |
| format | enum | no | "text" | "text" \| "markdown" | Output format |

**Execute logic**:
1. Extract `doc_id` (required) and `format` (default "text")
2. Call Feishu API: `GET /open-apis/docx/v1/documents/{doc_id}/raw_content`
3. If `format` is "markdown": convert Feishu block content to markdown
4. If `format` is "text": extract plain text
5. Return document content

---

### 52. feishu_drive_list_comments
- **Toolset**: comms
- **Description**: List comments on a Feishu document
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait (Feishu-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| doc_id | string | yes | — | — | Feishu document ID |

**Execute logic**:
1. Extract `doc_id` (required)
2. Call Feishu API: `GET /open-apis/drive/v1/files/{doc_id}/comments`
3. Parse comment list with author, content, timestamp, reply count
4. Format as: `"[{comment_id}] {author} ({time}): {content} ({reply_count} replies)"`
5. Return formatted list

---

### 53. feishu_drive_list_comment_replies
- **Toolset**: comms
- **Description**: List replies to a specific comment on a Feishu document
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait (Feishu-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| doc_id | string | yes | — | — | Feishu document ID |
| comment_id | string | yes | — | — | Comment ID to list replies for |

**Execute logic**:
1. Extract `doc_id` and `comment_id` (both required)
2. Call Feishu API: `GET /open-apis/drive/v1/files/{doc_id}/comments/{comment_id}/replies`
3. Parse reply list with author, content, timestamp
4. Format as: `"{author} ({time}): {content}"`
5. Return formatted reply list

---

### 54. feishu_drive_reply_comment
- **Toolset**: comms
- **Description**: Reply to an existing comment on a Feishu document
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait (Feishu-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| doc_id | string | yes | — | — | Feishu document ID |
| comment_id | string | yes | — | — | Comment ID to reply to |
| content | string | yes | — | — | Reply content |

**Execute logic**:
1. Extract `doc_id`, `comment_id`, and `content` (all required)
2. Call Feishu API: `POST /open-apis/drive/v1/files/{doc_id}/comments/{comment_id}/replies`
3. Return `"Reply posted to comment {comment_id}"`

---

### 55. feishu_drive_add_comment
- **Toolset**: comms
- **Description**: Add a new comment to a Feishu document
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `comms`
- **Provider**: `MessageProvider` trait (Feishu-specific instance)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| doc_id | string | yes | — | — | Feishu document ID |
| content | string | yes | — | — | Comment content |
| quote | string | no | — | — | Quoted text from the document (anchors the comment) |

**Execute logic**:
1. Extract `doc_id` and `content` (required), `quote` (optional)
2. Build comment payload; if `quote` provided, include as range anchor
3. Call Feishu API: `POST /open-apis/drive/v1/files/{doc_id}/comments`
4. Return `"Comment added to document {doc_id}"`

---

## Tier 9: IoT (4 tools) — feature = "iot"

All IoT tools belong to the `iot` toolset and require the `HomeProvider` trait (Home Assistant REST API backend).

### 56. ha_list_entities
- **Toolset**: iot
- **Description**: List all Home Assistant entities with optional domain and area filtering
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `iot`
- **Provider**: `HomeProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| domain | string | no | — | — | Filter by entity domain (e.g., "light", "switch", "sensor") |
| area | string | no | — | — | Filter by area name |

**Execute logic**:
1. Extract optional `domain` and `area` filters
2. Call `provider.list_entities()` — HA REST API: `GET /api/states`
3. Filter results:
   a. If `domain` provided: only include entities whose `entity_id` starts with `{domain}.`
   b. If `area` provided: only include entities belonging to that area (requires area registry lookup)
4. Format as table: `Entity ID | State | Friendly Name | Last Updated`
5. If no matching entities: return `"No entities found"`
6. Return formatted list

---

### 57. ha_get_state
- **Toolset**: iot
- **Description**: Get the current state and attributes of a Home Assistant entity
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `iot`
- **Provider**: `HomeProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| entity_id | string | yes | — | — | Entity ID (e.g., "light.living_room") |

**Execute logic**:
1. Extract `entity_id` (required)
2. Call `provider.get_state(entity_id)` — HA REST API: `GET /api/states/{entity_id}`
3. If entity not found: return error `"Entity '{entity_id}' not found"`
4. Format response: state value, all attributes, last changed/updated timestamps
5. Return formatted state information

---

### 58. ha_list_services
- **Toolset**: iot
- **Description**: List available Home Assistant services
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `iot`
- **Provider**: `HomeProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| domain | string | no | — | — | Filter by service domain (e.g., "light", "switch") |

**Execute logic**:
1. Extract optional `domain` filter
2. Call `provider.list_services()` — HA REST API: `GET /api/services`
3. If `domain` provided: filter to only that domain
4. Format as hierarchical list: `{domain}.{service}: {description}`
5. Include required and optional fields for each service
6. Return formatted service list

---

### 59. ha_call_service
- **Toolset**: iot
- **Description**: Call a Home Assistant service to control devices
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `iot`
- **Provider**: `HomeProvider` trait
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| domain | string | yes | — | — | Service domain (e.g., "light", "switch", "media_player") |
| service | string | yes | — | — | Service name (e.g., "turn_on", "turn_off", "toggle") |
| entity_id | string | no | — | — | Target entity ID |
| data | object | no | {} | — | Additional service data (e.g., brightness, color, temperature) |

**Execute logic**:
1. Extract `domain` and `service` (required), `entity_id` (optional), `data` (optional)
2. Build service call payload:
   - If `entity_id` provided: include as `target.entity_id`
   - Merge `data` into service data
3. Call `provider.call_service(domain, service, entity_id, data)` — HA REST API: `POST /api/services/{domain}/{service}`
4. Return `"Called {domain}.{service}" + entity info if applicable`

---

## Tier 10: Extra (7 tools) — feature = "extra"

### 60. cronjob
- **Toolset**: extra
- **Description**: Manage scheduled tasks with cron expressions
- **Permission**: Ask | **Read-only**: No | **Timeout**: 120s
- **Feature**: `extra`
- **Provider**: none (local filesystem + OS scheduler)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| action | enum | yes | — | "create" \| "list" \| "delete" \| "enable" \| "disable" | Action to perform |
| schedule | string | no | — | valid cron expr, required for "create" | Cron schedule expression (e.g., "0 * * * *") |
| command | string | no | — | required for "create" | Command to execute on schedule |
| job_id | string | no | — | required for delete/enable/disable | Job identifier |

**Execute logic**:

**Storage**: `{ctx.root}/.cold/cron/jobs.json`

**action = "create"**:
1. Extract `schedule` and `command` (both required)
2. Validate cron expression syntax
3. Generate job ID: `JOB-{sequential}`
4. Store job entry: `{ id, schedule, command, enabled: true, created_at }`
5. Register with OS scheduler if possible (crontab on Unix, Task Scheduler on Windows)
6. Return `"Created job {job_id}: '{command}' on schedule '{schedule}'"`

**action = "list"**:
7. Load jobs.json
8. Format as table: `ID | Schedule | Status | Command | Last Run`
9. Return formatted list

**action = "delete"**:
10. Extract `job_id` (required)
11. Remove job from storage and OS scheduler
12. Return `"Deleted job {job_id}"`

**action = "enable"** / **"disable"**:
13. Extract `job_id` (required)
14. Toggle enabled flag, update OS scheduler accordingly
15. Return `"Enabled/Disabled job {job_id}"`

---

### 61. mixture_of_agents
- **Toolset**: extra
- **Description**: Query multiple AI models and aggregate their responses
- **Permission**: Ask | **Read-only**: Yes | **Timeout**: 300s
- **Feature**: `extra`
- **Provider**: Multiple model clients (model router)
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| prompt | string | yes | — | — | Prompt to send to all models |
| models | array of strings | yes | — | min: 2 | List of model identifiers to query |
| aggregation | enum | no | "best" | "best" \| "merge" \| "vote" | How to aggregate responses |

**Execute logic**:
1. Extract `prompt` (required), `models` (required, min 2), `aggregation` (default "best")
2. Fan out: send `prompt` to each model in parallel
3. Collect all responses (with timeout per model)
4. Aggregate based on strategy:
   - `"best"`: use a judge model to pick the best response
   - `"merge"`: synthesize all responses into a unified answer
   - `"vote"`: use majority consensus for factual queries
5. Return aggregated result with source attribution

---

### 62. yb_query_group_info
- **Toolset**: extra
- **Description**: Query Yuanbao group information
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `extra`
- **Provider**: Yuanbao API client
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| group_id | string | yes | — | — | Yuanbao group ID |

**Execute logic**:
1. Extract `group_id` (required)
2. Call Yuanbao API to fetch group metadata
3. Return group info: name, description, member count, creation date

---

### 63. yb_query_group_members
- **Toolset**: extra
- **Description**: List members of a Yuanbao group
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `extra`
- **Provider**: Yuanbao API client
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| group_id | string | yes | — | — | Yuanbao group ID |

**Execute logic**:
1. Extract `group_id` (required)
2. Call Yuanbao API to list group members
3. Format as: `"{name} ({role}) - {user_id}"`
4. Return formatted member list

---

### 64. yb_send_dm
- **Toolset**: extra
- **Description**: Send a direct message via Yuanbao
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `extra`
- **Provider**: Yuanbao API client
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| user_id | string | yes | — | — | Recipient user ID |
| content | string | yes | — | — | Message content |

**Execute logic**:
1. Extract `user_id` and `content` (both required)
2. Call Yuanbao API to send direct message
3. Return `"Message sent to user {user_id}"`

---

### 65. yb_search_sticker
- **Toolset**: extra
- **Description**: Search for stickers in Yuanbao
- **Permission**: Auto | **Read-only**: Yes | **Timeout**: 30s
- **Feature**: `extra`
- **Provider**: Yuanbao API client
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| query | string | yes | — | — | Search query for stickers |

**Execute logic**:
1. Extract `query` (required)
2. Call Yuanbao API to search stickers by keyword
3. Format results: `"{sticker_id}: {name} ({pack_name})"`
4. Return formatted sticker list

---

### 66. yb_send_sticker
- **Toolset**: extra
- **Description**: Send a sticker via Yuanbao
- **Permission**: Ask | **Read-only**: No | **Timeout**: 30s
- **Feature**: `extra`
- **Provider**: Yuanbao API client
- **Status**: 📋 Planned

**Parameters**:
| Name | Type | Required | Default | Constraints | Description |
|------|------|----------|---------|-------------|-------------|
| user_id | string | yes | — | — | Recipient user ID |
| sticker_id | string | yes | — | — | Sticker ID to send |

**Execute logic**:
1. Extract `user_id` and `sticker_id` (both required)
2. Call Yuanbao API to send sticker to user
3. Return `"Sticker {sticker_id} sent to user {user_id}"`

---

## Skill System

The Skill System provides markdown-based prompt injection for domain expertise, coding patterns, and behavioral guidelines.

### Architecture

```
{ctx.root}/.cold/skills/
├── {skill-name}/
│   ├── SKILL.md          ← Main skill definition (frontmatter + content)
│   ├── .disabled          ← Marker file (presence = disabled)
│   └── assets/            ← Optional supporting files
```

### SKILL.md Format

```markdown
---
name: skill-name
version: 1.0.0
description: One-line description
triggers:
  - keyword1
  - keyword2
  - regex:pattern.*
priority: 100
inject: system        # system | user | tool_description
---

# Skill content here

This content is injected into the prompt when triggers match.
```

### Skill Lifecycle

1. **Discovery**: On startup, scan `{root}/.cold/skills/` for directories containing `SKILL.md`
2. **Indexing**: Parse SKILL.md frontmatter to build an in-memory skill index:
   - Key: trigger keywords/patterns
   - Value: skill name, file path, priority, inject mode
3. **Matching**: On each user message, check triggers against message content:
   - Exact keyword match (case-insensitive)
   - Regex pattern match (for `regex:` prefixed triggers)
   - Multiple skills can match; sorted by priority (higher = injected first)
4. **Injection**: Inject matched skill content into the prompt:
   - `inject: system` — prepend to system prompt
   - `inject: user` — append to user message as context
   - `inject: tool_description` — append to relevant tool descriptions
5. **Caching**: Skill content is cached after first read; invalidated on file modification

### Skill Management Tools

Skills are managed via the `skill_manage` (#14), `skills_list` (#15), and `skill_view` (#16) tools in Tier 2.

---

## MCP Bridge

The MCP (Model Context Protocol) Bridge allows external MCP servers to expose their tools as first-class citizens in the cold-tools registry. Once bridged, MCP tools are indistinguishable from built-in tools to the agent.

### Architecture

```
cold-tools Registry
    │
    ├── Built-in Tools (Tier 1-10)
    │
    └── MCP Bridge
        ├── MCP Client (stdio transport)
        │   └── → spawns MCP server subprocess
        │   └── → communicates via stdin/stdout JSON-RPC
        │
        └── MCP Client (SSE transport)
            └── → connects to HTTP SSE endpoint
            └── → communicates via HTTP POST + Server-Sent Events
```

### Transport: stdio

1. **Connection**: Spawn MCP server as subprocess with stdin/stdout pipes
2. **Initialization**: Send `initialize` request with client capabilities
3. **Discovery**: Send `tools/list` request to enumerate available tools
4. **Invocation**: Send `tools/call` request with tool name and arguments
5. **Lifecycle**: Keep subprocess alive for the session; kill on shutdown

### Transport: SSE (Server-Sent Events)

1. **Connection**: HTTP GET to SSE endpoint, receive event stream
2. **Initialization**: POST `initialize` request to server's message endpoint
3. **Discovery**: POST `tools/list` to enumerate tools
4. **Invocation**: POST `tools/call` for each tool invocation
5. **Lifecycle**: Maintain SSE connection; reconnect on disconnect

### Tool Trait Adapter

Each discovered MCP tool is wrapped in a struct that implements the `Tool` trait:

```rust
struct McpToolAdapter {
    name: String,
    description: String,
    schema: Value,              // from MCP tool definition
    client: Arc<McpClient>,     // shared client connection
}

impl Tool for McpToolAdapter {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn toolset(&self) -> &'static str { "mcp" }
    fn parameters_schema(&self) -> Value { self.schema.clone() }
    fn permission(&self) -> Permission { Permission::Ask }  // always Ask for external tools
    fn is_read_only(&self) -> bool { false }                // assume side effects
    fn timeout_secs(&self) -> u64 { 120 }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        // Send tools/call to MCP server, convert response to ToolResult
    }
}
```

### Registration Flow

1. Parse MCP server config (command + args for stdio, URL for SSE)
2. Establish transport connection
3. Send `initialize` + `tools/list`
4. For each tool: create `McpToolAdapter`, register in `ToolRegistry`
5. Increment registry generation (triggers tool list refresh for LLM)

### Error Handling

- Transport errors (pipe broken, HTTP timeout): return `ToolError::Execution` with details
- MCP protocol errors (invalid response): return `ToolError::Execution`
- Tool not found on server: return `ToolError::NotFound`
- Server crash: attempt reconnect once, then return error

---

## Implementation Batches

| Batch | Content | Tools | Status |
|-------|---------|-------|--------|
| 1 | Protocol + Core + Extended Core | 20 | ✅ Implemented (v0.1.0) |
| 2 | MCP Bridge | 2 | ✅ Implemented (v0.1.0) |
| 3 | Tier 7 Kanban | 9 | 📋 Planned (feature = "kanban") |
| 4 | Tier 4 Browser | 12 | 📋 Planned (feature = "browser") |
| 5 | Tier 6 Media + Tier 8 Comms + Tier 9 IoT | 17 | 📋 Planned (features) |
| 6 | Tier 5 Sandbox + Tier 10 Extra | 9 | 📋 Planned (features) |

### Batch 1 — Protocol + Core + Extended (✅ Implemented)
**Protocol Layer:**
- `Tool` trait with async execute, permission, read-only, timeout, concurrency_safe, should_defer, search_hints
- `ToolResult` with Text/Json/Error/Empty variants and safe truncation
- `ToolRegistry` with register/deregister/dispatch/definitions/register_auto
- `ToolContext` with cwd/root/user/cancelled/env/plan_mode
- `Schema` builder for type-safe JSON Schema construction
- Security: path validation, traversal detection, dangerous command detection, sandbox
- Permissions: 6 modes + rule matching + denial tracking + decision reasons
- Guardrails: loop detection, same-tool halt, no-progress block
- Dispatcher: JoinSet parallel execution + sibling abort
- Result storage: large results persisted to disk
- Result budget: per-message aggregate truncation
- Deferred registry: ToolSearch + keyword scoring

**20 Tools:**
read_file, write_file, edit_file, search_files, glob, list_dir, terminal, process, think, ask_user, todo_write, notebook_edit, enter_plan_mode, exit_plan_mode, web_fetch, web_search, tool_search, mcp_list_resources, mcp_read_resource

**Providers:** WebProvider trait (fetch + search)

### Batch 2 — MCP Bridge (✅ Implemented)
- McpTransport trait (list_tools, call_tool, list_resources, read_resource)
- McpToolAdapter implementing Tool trait
- register_mcp_tools() for bulk registration

### Batch 3 — Kanban (📋 Planned, feature = "kanban")
- 9 kanban tools with local JSON storage
- Board visualization, task lifecycle, comments, links

### Batch 4 — Browser (📋 Planned, feature = "browser")
- BrowserProvider trait (CDP over WebSocket)
- 12 browser automation tools

### Batch 5 — Media + Comms + IoT (📋 Planned, features)
- MediaProvider, MessageProvider, HomeProvider traits
- 5 media + 8 comms + 4 IoT tools

### Batch 6 — Sandbox + Extra (📋 Planned, features)
- execute_code, computer_use, cronjob, mixture_of_agents
