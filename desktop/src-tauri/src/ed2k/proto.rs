//! eD2k / eMule 协议的报文编解码：报文头、标签、以及与服务器、对等端交互用到的各条消息。
//! 数字都是小端；报文 = 协议字节(1) + 长度(4，含操作码) + 操作码(1) + 数据。

use std::io::{self, Cursor};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::hash::Hash;

pub const PROTO_EDONKEY: u8 = 0xE3;
pub const PROTO_EMULE: u8 = 0xC5;
pub const PROTO_PACKED: u8 = 0xD4;

/// 单个报文的长度上限，防止对方发来畸形数据撑爆内存。
pub const MAX_PACKET: usize = 4 * 1024 * 1024;

// 客户端 → 服务器
pub const OP_LOGINREQUEST: u8 = 0x01;
pub const OP_GETSOURCES: u8 = 0x19;
// 服务器 → 客户端
pub const OP_SERVERSTATUS: u8 = 0x34;
pub const OP_SERVERMESSAGE: u8 = 0x38;
pub const OP_IDCHANGE: u8 = 0x40;
pub const OP_FOUNDSOURCES: u8 = 0x42;
// 客户端 ↔ 客户端
pub const OP_HELLO: u8 = 0x01;
pub const OP_SENDINGPART: u8 = 0x46;
pub const OP_REQUESTPARTS: u8 = 0x47;
pub const OP_FILEREQANSNOFIL: u8 = 0x48;
pub const OP_END_OF_DOWNLOAD: u8 = 0x49;
pub const OP_SETREQFILEID: u8 = 0x4F;
pub const OP_FILESTATUS: u8 = 0x50;
pub const OP_HASHSETREQUEST: u8 = 0x51;
pub const OP_HASHSETANSWER: u8 = 0x52;
pub const OP_STARTUPLOADREQ: u8 = 0x54;
pub const OP_ACCEPTUPLOADREQ: u8 = 0x55;
pub const OP_CANCELTRANSFER: u8 = 0x56;
pub const OP_OUTOFPARTREQS: u8 = 0x57;
pub const OP_FILEREQUEST: u8 = 0x58;
pub const OP_FILEREQANSWER: u8 = 0x59;
pub const OP_HELLOANSWER: u8 = 0x4C;
pub const OP_QUEUERANK: u8 = 0x5C;
// eMule 扩展协议（0xC5）
pub const OP_COMPRESSEDPART: u8 = 0x40;
pub const OP_QUEUERANKING: u8 = 0x60;
pub const OP_SENDINGPART_I64: u8 = 0xA2;
pub const OP_REQUESTPARTS_I64: u8 = 0xA3;

// 标签
pub const CT_NAME: u8 = 0x01;
pub const CT_PORT: u8 = 0x0F;
pub const CT_VERSION: u8 = 0x11;
pub const CT_SERVER_FLAGS: u8 = 0x20;

const TAG_HASH: u8 = 0x01;
const TAG_STRING: u8 = 0x02;
const TAG_UINT32: u8 = 0x03;
const TAG_FLOAT: u8 = 0x04;
const TAG_BOOL: u8 = 0x05;
const TAG_BOOLARRAY: u8 = 0x06;
const TAG_BLOB: u8 = 0x07;
const TAG_UINT16: u8 = 0x08;
const TAG_UINT8: u8 = 0x09;
const TAG_BSOB: u8 = 0x0A;
const TAG_UINT64: u8 = 0x0B;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub proto: u8,
    pub opcode: u8,
    pub data: Vec<u8>,
}

impl Packet {
    pub fn new(opcode: u8, data: Vec<u8>) -> Packet {
        Packet { proto: PROTO_EDONKEY, opcode, data }
    }

    pub fn emule(opcode: u8, data: Vec<u8>) -> Packet {
        Packet { proto: PROTO_EMULE, opcode, data }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.data.len());
        out.push(self.proto);
        out.extend_from_slice(&(self.data.len() as u32 + 1).to_le_bytes());
        out.push(self.opcode);
        out.extend_from_slice(&self.data);
        out
    }
}

pub async fn read_packet<R: AsyncRead + Unpin>(r: &mut R) -> io::Result<Packet> {
    let mut head = [0u8; 6];
    r.read_exact(&mut head).await?;
    let proto = head[0];
    if !matches!(proto, PROTO_EDONKEY | PROTO_EMULE | PROTO_PACKED) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("不是 eD2k 报文（协议字节 0x{proto:02X}）")));
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len == 0 || len > MAX_PACKET {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("报文长度不合理：{len}")));
    }
    let mut data = vec![0u8; len - 1];
    r.read_exact(&mut data).await?;
    Ok(Packet { proto, opcode: head[5], data })
}

pub async fn write_packet<W: AsyncWrite + Unpin>(w: &mut W, p: &Packet) -> io::Result<()> {
    w.write_all(&p.encode()).await?;
    w.flush().await
}

// ---------- 读写助手 ----------

#[derive(Default)]
pub struct Out(pub Vec<u8>);

impl Out {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.u16(s.len().min(u16::MAX as usize) as u16);
        self.bytes(&s.as_bytes()[..s.len().min(u16::MAX as usize)])
    }
    /// 标签：字符串（老式写法：类型 + 名称长度 + 名称字节）
    pub fn tag_str(&mut self, id: u8, v: &str) -> &mut Self {
        self.u8(TAG_STRING).u16(1).u8(id).str(v)
    }
    pub fn tag_u32(&mut self, id: u8, v: u32) -> &mut Self {
        self.u8(TAG_UINT32).u16(1).u8(id).u32(v)
    }
}

pub struct In<'a> {
    c: Cursor<&'a [u8]>,
}

impl<'a> In<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        In { c: Cursor::new(data) }
    }
    pub fn left(&self) -> usize {
        self.c.get_ref().len() - self.c.position() as usize
    }
    fn arr<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut b = [0u8; N];
        std::io::Read::read_exact(&mut self.c, &mut b).map_err(|_| io::Error::new(io::ErrorKind::UnexpectedEof, "报文内容不完整"))?;
        Ok(b)
    }
    pub fn u8(&mut self) -> io::Result<u8> {
        Ok(self.arr::<1>()?[0])
    }
    pub fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    pub fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    pub fn hash(&mut self) -> io::Result<Hash> {
        self.arr()
    }
    pub fn take(&mut self, n: usize) -> io::Result<Vec<u8>> {
        if n > self.left() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "报文内容不完整"));
        }
        let mut v = vec![0u8; n];
        std::io::Read::read_exact(&mut self.c, &mut v)?;
        Ok(v)
    }
    pub fn rest(&mut self) -> Vec<u8> {
        let n = self.left();
        self.take(n).unwrap_or_default()
    }
    pub fn str(&mut self) -> io::Result<String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(&self.take(n)?).into_owned())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TagValue {
    Str(String),
    Num(u64),
    Hash(Hash),
    Float(f32),
    Blob(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    /// 数字 ID（名称只有一个字节时）；字符串名称的标签为 None
    pub id: Option<u8>,
    pub name: String,
    pub value: TagValue,
}

/// 读取标签列表（前面是 u32 个数）。老式和新式（类型高位为 1，名称是单字节 ID）两种写法都支持。
pub fn read_tags(r: &mut In) -> io::Result<Vec<Tag>> {
    let n = r.u32()?;
    if n > 256 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "标签数量不合理"));
    }
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let mut ty = r.u8()?;
        let (id, name) = if ty & 0x80 != 0 {
            ty &= 0x7F;
            let id = r.u8()?;
            (Some(id), String::new())
        } else {
            let len = r.u16()? as usize;
            let nb = r.take(len)?;
            if len == 1 {
                (Some(nb[0]), String::new())
            } else {
                (None, String::from_utf8_lossy(&nb).into_owned())
            }
        };
        let value = match ty {
            TAG_HASH => TagValue::Hash(r.hash()?),
            TAG_STRING => TagValue::Str(r.str()?),
            TAG_UINT32 => TagValue::Num(r.u32()? as u64),
            TAG_UINT16 => TagValue::Num(r.u16()? as u64),
            TAG_UINT8 => TagValue::Num(r.u8()? as u64),
            TAG_UINT64 => TagValue::Num(r.u64()?),
            TAG_FLOAT => TagValue::Float(f32::from_le_bytes(r.arr()?)),
            TAG_BOOL => TagValue::Num(r.u8()? as u64),
            TAG_BOOLARRAY => {
                let n = r.u16()? as usize;
                r.take(n.div_ceil(8))?;
                TagValue::Num(0)
            }
            TAG_BLOB => {
                let n = r.u32()? as usize;
                TagValue::Blob(r.take(n)?)
            }
            TAG_BSOB => {
                let n = r.u8()? as usize;
                TagValue::Blob(r.take(n)?)
            }
            0x11..=0x20 => TagValue::Str(String::from_utf8_lossy(&r.take((ty - 0x10) as usize)?).into_owned()),
            other => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("未知的标签类型 0x{other:02X}"))),
        };
        out.push(Tag { id, name, value });
    }
    Ok(out)
}

// ---------- 与服务器 ----------

/// 登录请求。ID 填 0，由服务器根据连接情况分配；端口是我们的监听端口（不接受入站连接时也要填一个）。
pub fn login_request(user_hash: &Hash, port: u16, name: &str) -> Packet {
    let mut o = Out::default();
    o.bytes(user_hash).u32(0).u16(port).u32(3);
    o.tag_str(CT_NAME, name).tag_u32(CT_VERSION, 0x3C).tag_u32(CT_SERVER_FLAGS, 0);
    Packet::new(OP_LOGINREQUEST, o.0)
}

/// 服务器分配的 ID；小于 0x01000000 的是低 ID（无法被直接连接，需要服务器回调）。
pub fn parse_id_change(data: &[u8]) -> io::Result<u32> {
    In::new(data).u32()
}

pub fn is_low_id(id: u32) -> bool {
    id < 0x0100_0000
}

pub fn get_sources(hash: &Hash, size: u64) -> Packet {
    let mut o = Out::default();
    o.bytes(hash);
    if size <= u32::MAX as u64 {
        o.u32(size as u32);
    } else {
        o.u32(0).u64(size);
    }
    Packet::new(OP_GETSOURCES, o.0)
}

/// 服务器回复里的来源：（文件哈希，地址）。低 ID 的来源没有可连接的地址，返回时跳过。
pub fn parse_found_sources(data: &[u8]) -> io::Result<(Hash, Vec<SocketAddr>)> {
    let mut r = In::new(data);
    let hash = r.hash()?;
    let n = r.u8()? as usize;
    let mut out = vec![];
    for _ in 0..n {
        let ip = r.take(4)?;
        let port = r.u16()?;
        if ip[3] == 0 || port == 0 {
            continue;
        }
        out.push(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]), port)));
    }
    Ok((hash, out))
}

pub fn found_sources(hash: &Hash, sources: &[SocketAddrV4]) -> Packet {
    let mut o = Out::default();
    o.bytes(hash).u8(sources.len().min(255) as u8);
    for s in sources.iter().take(255) {
        o.bytes(&s.ip().octets()).u16(s.port());
    }
    Packet::new(OP_FOUNDSOURCES, o.0)
}

// ---------- 与对等端 ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Hello {
    pub user_hash: Hash,
    pub client_id: u32,
    pub port: u16,
    pub name: String,
}

fn hello_body(o: &mut Out, user_hash: &Hash, port: u16, name: &str) {
    o.bytes(user_hash).u32(0).u16(port).u32(2);
    o.tag_str(CT_NAME, name).tag_u32(CT_VERSION, 0x3C);
    // 所在服务器：不告诉对方
    o.u32(0).u16(0);
}

pub fn hello(user_hash: &Hash, port: u16, name: &str) -> Packet {
    let mut o = Out::default();
    o.u8(16);
    hello_body(&mut o, user_hash, port, name);
    Packet::new(OP_HELLO, o.0)
}

pub fn hello_answer(user_hash: &Hash, port: u16, name: &str) -> Packet {
    let mut o = Out::default();
    hello_body(&mut o, user_hash, port, name);
    Packet::new(OP_HELLOANSWER, o.0)
}

/// 解析 HELLO（带 16 的长度字节）或 HELLOANSWER。
pub fn parse_hello(data: &[u8], with_len_byte: bool) -> io::Result<Hello> {
    let mut r = In::new(data);
    if with_len_byte {
        let n = r.u8()?;
        if n != 16 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "HELLO 里的用户哈希长度不是 16"));
        }
    }
    let user_hash = r.hash()?;
    let client_id = r.u32()?;
    let port = r.u16()?;
    let tags = read_tags(&mut r)?;
    let name =
        tags.iter().find(|t| t.id == Some(CT_NAME)).and_then(|t| if let TagValue::Str(s) = &t.value { Some(s.clone()) } else { None }).unwrap_or_default();
    Ok(Hello { user_hash, client_id, port, name })
}

pub fn file_request(op: u8, hash: &Hash) -> Packet {
    Packet::new(op, hash.to_vec())
}

pub fn file_req_answer(hash: &Hash, name: &str) -> Packet {
    let mut o = Out::default();
    o.bytes(hash).str(name);
    Packet::new(OP_FILEREQANSWER, o.0)
}

pub fn parse_file_req_answer(data: &[u8]) -> io::Result<(Hash, String)> {
    let mut r = In::new(data);
    Ok((r.hash()?, r.str()?))
}

/// 对方有哪些分块。`None` 表示对方有完整文件（分块数为 0）。
pub fn file_status(hash: &Hash, parts: Option<&[bool]>) -> Packet {
    let mut o = Out::default();
    o.bytes(hash);
    match parts {
        None => {
            o.u16(0);
        }
        Some(p) => {
            o.u16(p.len() as u16);
            let mut bytes = vec![0u8; p.len().div_ceil(8)];
            for (i, has) in p.iter().enumerate() {
                if *has {
                    bytes[i / 8] |= 1 << (i % 8);
                }
            }
            o.bytes(&bytes);
        }
    }
    Packet::new(OP_FILESTATUS, o.0)
}

pub fn parse_file_status(data: &[u8], total_parts: usize) -> io::Result<(Hash, Vec<bool>)> {
    let mut r = In::new(data);
    let hash = r.hash()?;
    let n = r.u16()? as usize;
    if n == 0 {
        return Ok((hash, vec![true; total_parts]));
    }
    let bytes = r.take(n.div_ceil(8))?;
    let has = (0..total_parts).map(|i| i < n && bytes[i / 8] & (1 << (i % 8)) != 0).collect();
    Ok((hash, has))
}

pub fn hashset_answer(hash: &Hash, parts: &[Hash]) -> Packet {
    let mut o = Out::default();
    o.bytes(hash).u16(parts.len() as u16);
    for p in parts {
        o.bytes(p);
    }
    Packet::new(OP_HASHSETANSWER, o.0)
}

pub fn parse_hashset_answer(data: &[u8]) -> io::Result<(Hash, Vec<Hash>)> {
    let mut r = In::new(data);
    let hash = r.hash()?;
    let n = r.u16()? as usize;
    let mut v = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        v.push(r.hash()?);
    }
    Ok((hash, v))
}

/// 请求最多三段数据（区间是 [起, 止)）。文件超过 4GB 时用 64 位版本的报文。
pub fn request_parts(hash: &Hash, ranges: &[(u64, u64)], large: bool) -> Packet {
    let mut r3 = [(0u64, 0u64); 3];
    for (i, r) in ranges.iter().take(3).enumerate() {
        r3[i] = *r;
    }
    let mut o = Out::default();
    o.bytes(hash);
    if large {
        for r in &r3 {
            o.u64(r.0);
        }
        for r in &r3 {
            o.u64(r.1);
        }
        Packet::emule(OP_REQUESTPARTS_I64, o.0)
    } else {
        for r in &r3 {
            o.u32(r.0 as u32);
        }
        for r in &r3 {
            o.u32(r.1 as u32);
        }
        Packet::new(OP_REQUESTPARTS, o.0)
    }
}

pub fn parse_request_parts(data: &[u8], large: bool) -> io::Result<(Hash, Vec<(u64, u64)>)> {
    let mut r = In::new(data);
    let hash = r.hash()?;
    let mut starts = [0u64; 3];
    let mut ends = [0u64; 3];
    for s in &mut starts {
        *s = if large { r.u64()? } else { r.u32()? as u64 };
    }
    for e in &mut ends {
        *e = if large { r.u64()? } else { r.u32()? as u64 };
    }
    Ok((hash, (0..3).filter(|i| ends[*i] > starts[*i]).map(|i| (starts[i], ends[i])).collect()))
}

pub fn sending_part(hash: &Hash, start: u64, data: &[u8], large: bool) -> Packet {
    let mut o = Out::default();
    o.bytes(hash);
    if large {
        o.u64(start).u64(start + data.len() as u64).bytes(data);
        Packet::emule(OP_SENDINGPART_I64, o.0)
    } else {
        o.u32(start as u32).u32((start + data.len() as u64) as u32).bytes(data);
        Packet::new(OP_SENDINGPART, o.0)
    }
}

/// 收到的数据块：（哈希，起点，数据）。
pub fn parse_sending_part(data: &[u8], large: bool) -> io::Result<(Hash, u64, Vec<u8>)> {
    let mut r = In::new(data);
    let hash = r.hash()?;
    let (start, end) = if large { (r.u64()?, r.u64()?) } else { (r.u32()? as u64, r.u32()? as u64) };
    if end < start || end - start != r.left() as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "数据块的长度与区间不符"));
    }
    Ok((hash, start, r.rest()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: Hash = [0xAB; 16];

    #[tokio::test]
    async fn packet_roundtrip_and_limits() {
        let p = Packet::new(OP_FILEREQUEST, H.to_vec());
        let enc = p.encode();
        assert_eq!(enc[0], PROTO_EDONKEY);
        assert_eq!(u32::from_le_bytes([enc[1], enc[2], enc[3], enc[4]]), 17);
        assert_eq!(enc[5], OP_FILEREQUEST);
        let mut c = Cursor::new(enc);
        let mut rd = tokio::io::BufReader::new(&mut c);
        assert_eq!(read_packet(&mut rd).await.unwrap(), p);
        // 畸形：协议字节不对、长度为 0、长度过大
        for bad in [vec![0x11, 1, 0, 0, 0, 1], vec![0xE3, 0, 0, 0, 0, 1], vec![0xE3, 0xFF, 0xFF, 0xFF, 0x7F, 1]] {
            let mut c = Cursor::new(bad);
            assert!(read_packet(&mut c).await.is_err());
        }
        // 截断
        let mut c = Cursor::new(vec![0xE3, 17, 0, 0, 0, 0x58, 1, 2]);
        assert!(read_packet(&mut c).await.is_err());
    }

    #[test]
    fn tags_in_both_encodings() {
        let mut o = Out::default();
        o.u32(5);
        o.tag_str(CT_NAME, "hello").tag_u32(CT_VERSION, 60);
        // 新式写法：类型 | 0x80 + 单字节 ID
        o.u8(TAG_UINT32 | 0x80).u8(0xFB).u32(0x0102_0304);
        // 字符串名称 + 短字符串类型
        o.u8(TAG_UINT8).u16(4).bytes(b"nick").u8(7);
        o.u8(0x15).u16(1).u8(0x30).bytes(b"abcde");
        let tags = read_tags(&mut In::new(&o.0)).unwrap();
        assert_eq!(tags.len(), 5);
        assert_eq!(tags[0], Tag { id: Some(CT_NAME), name: String::new(), value: TagValue::Str("hello".into()) });
        assert_eq!(tags[2].value, TagValue::Num(0x0102_0304));
        assert_eq!(tags[3].name, "nick");
        assert_eq!(tags[4].value, TagValue::Str("abcde".into()));
        // 未知类型 / 截断
        let mut bad = Out::default();
        bad.u32(1).u8(0x7E).u16(1).u8(1);
        assert!(read_tags(&mut In::new(&bad.0)).is_err());
        assert!(read_tags(&mut In::new(&[1, 0, 0, 0, TAG_STRING, 1, 0])).is_err());
    }

    #[test]
    fn hello_roundtrip() {
        let uh = [9u8; 16];
        let h = parse_hello(&hello(&uh, 4662, "我的客户端").data, true).unwrap();
        assert_eq!(h, Hello { user_hash: uh, client_id: 0, port: 4662, name: "我的客户端".into() });
        let a = parse_hello(&hello_answer(&uh, 4662, "x").data, false).unwrap();
        assert_eq!(a.name, "x");
        assert!(parse_hello(&hello(&uh, 1, "x").data[1..], true).is_err());
    }

    #[test]
    fn server_messages() {
        let login = login_request(&[1; 16], 4662, "ClearClip");
        assert_eq!(login.opcode, OP_LOGINREQUEST);
        let mut r = In::new(&login.data);
        assert_eq!(r.hash().unwrap(), [1; 16]);
        assert_eq!((r.u32().unwrap(), r.u16().unwrap()), (0, 4662));
        assert_eq!(read_tags(&mut r).unwrap().len(), 3);
        assert!(is_low_id(5) && is_low_id(0x00FF_FFFF) && !is_low_id(0x0100_0000));

        let gs = get_sources(&H, 1234);
        assert_eq!(gs.data.len(), 20);
        assert_eq!(get_sources(&H, 5_000_000_000).data.len(), 28);

        let fs = found_sources(&H, &["1.2.3.4:4662".parse().unwrap(), "10.0.0.9:4672".parse().unwrap()]);
        let (h, addrs) = parse_found_sources(&fs.data).unwrap();
        assert_eq!(h, H);
        assert_eq!(addrs, vec!["1.2.3.4:4662".parse::<SocketAddr>().unwrap(), "10.0.0.9:4672".parse().unwrap()]);
        // 低 ID（最后一个字节为 0）和端口 0 的来源被跳过
        let mut o = Out::default();
        o.bytes(&H).u8(2).bytes(&[5, 0, 0, 0]).u16(4662).bytes(&[8, 8, 8, 8]).u16(0);
        assert!(parse_found_sources(&o.0).unwrap().1.is_empty());
        assert!(parse_found_sources(&H).is_err());
    }

    #[test]
    fn file_status_bitmap() {
        let p = file_status(&H, Some(&[true, false, true, true, false, false, false, false, true]));
        let (h, has) = parse_file_status(&p.data, 9).unwrap();
        assert_eq!(h, H);
        assert_eq!(has, vec![true, false, true, true, false, false, false, false, true]);
        // 对方有完整文件
        let full = file_status(&H, None);
        assert_eq!(parse_file_status(&full.data, 4).unwrap().1, vec![true; 4]);
        // 对方报告的分块比我们少：多出的当作没有
        let short = file_status(&H, Some(&[true, true]));
        assert_eq!(parse_file_status(&short.data, 3).unwrap().1, vec![true, true, false]);
    }

    #[test]
    fn hashset_and_parts() {
        let set = [[1u8; 16], [2u8; 16], [3u8; 16]];
        assert_eq!(parse_hashset_answer(&hashset_answer(&H, &set).data).unwrap(), (H, set.to_vec()));

        let ranges = [(0, 184_320), (184_320, 368_640)];
        let rq = request_parts(&H, &ranges, false);
        assert_eq!(rq.opcode, OP_REQUESTPARTS);
        assert_eq!(parse_request_parts(&rq.data, false).unwrap(), (H, ranges.to_vec()));
        let big = request_parts(&H, &[(5_000_000_000, 5_000_184_320)], true);
        assert_eq!((big.proto, big.opcode), (PROTO_EMULE, OP_REQUESTPARTS_I64));
        assert_eq!(parse_request_parts(&big.data, true).unwrap().1, vec![(5_000_000_000, 5_000_184_320)]);

        let data = vec![5u8; 1000];
        let sp = sending_part(&H, 2000, &data, false);
        assert_eq!(parse_sending_part(&sp.data, false).unwrap(), (H, 2000, data.clone()));
        let spl = sending_part(&H, 6_000_000_000, &data, true);
        assert_eq!(parse_sending_part(&spl.data, true).unwrap().1, 6_000_000_000);
        // 区间长度与数据不符
        let mut broken = sp.data.clone();
        broken.pop();
        assert!(parse_sending_part(&broken, false).is_err());
    }
}
