# 让大模型 / 智能体调用清影（MCP）

`clearclip mcp` 是一个标准的 MCP（Model Context Protocol）服务，走标准输入输出。Claude Code、Claude Desktop、Codex、Cursor 等都用同一种方式接入：填一条启动命令即可，不需要为每个智能体单独做工具。

它只是清影本机接口（`/api/v1`）外面的一层包装，不监听端口，也不暴露到局域网。

## 先决条件

1. 清影桌面程序正在运行。
2. 在 **设置 → 手机与浏览器扩展** 里打开服务（MCP 用同一个端口和访问令牌，从设置文件里自动读取，不需要手填）。

## 配置

下面的 `clearclip` 换成安装目录里 `clearclip` 的完整路径（Windows 例：`C:\Program Files\ClearClip\clearclip.exe`）。只想让它查询、不让它下载或改动，在 `mcp` 后面加 `--read-only`。

**Claude Code**

```
claude mcp add clearclip -- clearclip mcp
```

**Claude Desktop**（`claude_desktop_config.json`）/ **Cursor**（`.cursor/mcp.json`）

```json
{
  "mcpServers": {
    "clearclip": { "command": "clearclip", "args": ["mcp"] }
  }
}
```

**Codex**（`~/.codex/config.toml`）

```toml
[mcp_servers.clearclip]
command = "clearclip"
args = ["mcp"]
```

## 工具

| 工具 | 作用 | 只读模式 |
| --- | --- | --- |
| `get_status` | 版本、各状态任务数量、总速度 | ✅ |
| `list_tasks` | 下载任务列表（可按状态筛选） | ✅ |
| `search_library` | 搜索媒体库（标题、作者、标签、来源链接） | ✅ |
| `get_subtitle_job` | 查询字幕任务进度 | ✅ |
| `add_download` | 提交一个或多个链接下载 | 不开放 |
| `pause_tasks` / `resume_tasks` | 暂停 / 继续（不传编号是全部） | 不开放 |
| `cancel_task` | 取消一个下载任务 | 不开放 |
| `generate_subtitle` | 给音视频生成字幕（可翻译、选格式），异步 | 不开放 |
| `correct_subtitle` | 字幕 AI 纠错，另存新文件，异步 | 不开放 |

## 边界

- 没有删除文件、移除任务、读写设置的工具；接口本来也不返回本机文件路径。
- 字幕工具的 `path` 必须是本机已存在的绝对路径；相对路径或不存在的文件在本地就被拒绝，不会发给清影。
- 标准输出只写协议消息；清影没开或没开启服务时，工具返回错误说明，不会让智能体卡住。
- 想让智能体“看”某个网站、登录状态等，仍要先在清影里同步登录（见 `docs/login-guide.md`）。
