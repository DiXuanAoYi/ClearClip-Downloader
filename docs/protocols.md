# 协议扩展（0.7.0）

清影可以下载网页视频、HTTP 文件，现在还支持下面这些链接。**直接粘贴到“添加下载”或复制到剪贴板**即可，不需要选择协议。

| 链接 | 说明 |
|---|---|
| `ftp://user:pass@host/path/file` | FTP，断点续传。账号密码写在链接里（可省略，匿名登录）。暂不支持 FTPS |
| `sftp://user:pass@host/path/file` | SFTP（SSH），支持密码或本机 `~/.ssh` 无口令私钥。首次连接记住服务器指纹（保存在数据目录的 `sftp_known_hosts.json`），指纹变化会拒绝连接 |
| `thunder://` `flashget://` `qqdl://` | 解码成原链接后按原链接的类型下载 |
| `rtmp://` `rtsp://` `mms://` 等 | 交给 ffmpeg 抓取 |
| `ed2k://\|file\|名称\|大小\|MD4\|/` | ed2k，见下 |
| `magnet:?xt=urn:btih:…` 或 `.torrent` 地址 | BT，见下 |

## BT / 磁力

- 内置 librqbit：DHT、PEX、UPnP。下载完成后**不做种**。
- 多文件的种子会展开成列表，勾选要下的文件；每个文件是一个下载任务，同一种子里的文件依次下载。
- Tracker 列表：在“设置 → BT 与 ed2k”里管理。
  - 默认每 7 天更新，间隔可改。列表来源按顺序尝试、第一个成功的为准，默认依次是：ngosang/trackerslist 的 best 列表（jsDelivr，再试 GitHub）、`https://trackerslist.com/best.txt`、`https://cf.trackerslist.com/best.txt`、XIU2/TrackersListCollection 的 `best.txt`；可在“BT 与 ed2k → 列表来源”里增删、调整顺序。旧版本里没改过默认来源的，升级后自动补上新增的。
  - 可以“立即更新”，也可以增删列表来源，或添加自己的 tracker（始终使用，排在列表前面）。
  - 更新失败会保留上次的列表；只接受 udp / http / https 的地址，自动去重。
- 没有内置搜索，需要自己找到 magnet 链接。
- 在没有 IPv6 的系统（部分内核、容器）上，librqbit 的 UDP tracker 可能无法启动，错误提示里会说明。

## ed2k

- 自行实现的协议（MD4、服务器协议、来源直连、分块传输与校验），没有依赖现成的 ed2k 库。
- 来源来自链接里携带的地址和已配置的服务器；可在设置里填服务器地址，或填一个 server.met 地址（失败时使用上次缓存）。
- 按 9.28 MB 分块下载，每块 MD4 校验，校验失败自动换来源重下；断点续传（进度保存在 `.ed2k` 旁路文件里）。
- 不做：搜索、Kad 网络、压缩分块、低 ID 来源（需要对方回连）、上传共享。公开的 ed2k 服务器已经很少，能不能下取决于服务器和来源。

## 已知限制

- BT：没有做种、不支持代理、不支持直接拖入本地 `.torrent` 文件（用 http(s) 的 .torrent 地址或 magnet）。
- FTP 不支持 FTPS；SFTP 的私钥登录只支持本机 `~/.ssh` 里没有口令的私钥。
- 本版本的开发环境没有真实的 BT / ed2k 网络，这两部分用本地模拟的 peer / 服务器和本地 tracker 做过测试，真实网络下的表现需要在使用中反馈。
