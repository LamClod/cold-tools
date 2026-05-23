<p align="center">
  <h1 align="center">cold-tools</h1>
  <p align="center">LAMCLOD 工具协议框架与内置工具集</p>
  <p align="center">
    <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square" alt="Rust">
    <img src="https://img.shields.io/badge/tests-50_pass-brightgreen?style=flat-square" alt="tests">
    <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT">
  </p>
</p>

---

## 简介

cold-tools 是 LAMCLOD 的工具协议框架，定义工具接口、注册调度、权限控制、安全保护，并提供 17 个内置工具。

## 特性

| | |
|---|---|
| **Tool Trait** | 统一工具接口：schema + execute + 权限 + 超时 + 并行 |
| **Registry** | 注册/注销/调度/OpenAI 格式 schema 生成 |
| **17 内置工具** | read/write/edit/search/glob/list/terminal/process/think/ask/todo/notebook/web/plan_mode |
| **6 级权限** | Default / AcceptEdits / BypassPermissions / DontAsk / Plan / Auto + 规则匹配 |
| **MCP 桥接** | McpTransport trait + McpToolAdapter，外部工具平等调度 |
| **安全** | 路径验证 + 危险命令检测 + sandbox + 输出截断 |
| **并行执行** | JoinSet 真并行 + sibling abort |
| **延迟加载** | ToolSearch + DeferredRegistry |

## 安装

```toml
[dependencies]
cold-tools = "0.1"
```

## 用法

```rust
use cold_tools::{ToolRegistry, CoreToolConfig, register_core_tools, ToolContext, AutoApprove};

let mut registry = ToolRegistry::new();
register_core_tools(&mut registry, CoreToolConfig::default());

// 调度工具
let ctx = ToolContext::new(".", ".", "session-1");
let result = registry.dispatch("read_file", serde_json::json!({"path": "src/main.rs"}), &ctx).await?;
```

## Cold Stack

cold-cli 是 LAMCLOD 的 AI 编码助手 CLI，基于以下 4 个 Rust crate 构建：

```
cold-cli              CLI 入口
  |
cold-agent-sdk        Agent 编排 (loop + sub-agent + hooks + memory)
  |
  +-- cold-context    上下文管理 (压缩 + 安全 + 预算)
  +-- cold-tools      工具框架 + 20 内置工具 + MCP
  |
cold-sdk              API 传输层 (HTTP/2 + SSE + 重试)
```

| Crate | 描述 |
|-------|------|
| [cold-sdk](https://github.com/LamClod/cold-sdk) | API 通信层 |
| [cold-context](https://github.com/LamClod/cold-context) | 上下文窗口管理 |
| [cold-tools](https://github.com/LamClod/cold-tools) | 工具协议框架 |
| [cold-agent-sdk](https://github.com/LamClod/cold-agent-sdk) | Agent 编排 SDK |
| [cold-cli](https://github.com/LamClod/cold-cli) | 命令行界面 |

## License

MIT
