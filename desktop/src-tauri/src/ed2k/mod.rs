//! eD2k（电驴）协议：链接解析、MD4 哈希、服务器查询来源、对等端分块下载。
//! 没有内置的资源索引或搜索：有 ed2k 链接就能下载，来源从链接、服务器取得。

pub mod download;
pub mod hash;
pub mod link;
pub mod md4;
pub mod peer;
pub mod proto;
pub mod sched;
pub mod server;
