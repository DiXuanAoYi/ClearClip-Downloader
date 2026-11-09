//! 腾讯云、阿里云、火山引擎三家机器翻译的请求签名（都是 HMAC-SHA256 的云厂商签名）。
//! 这里只放纯函数：给定时间、密钥和请求内容，算出要带的请求头，方便单独测试；真正发请求在 mt.rs。

use sha2::{Digest, Sha256};

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// HMAC-SHA256（RFC 2104），不用再多引一个依赖。
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 64;
    let mut k = if key.len() > BLOCK { Sha256::digest(key).to_vec() } else { key.to_vec() };
    k.resize(BLOCK, 0);
    let mut inner = Sha256::new();
    inner.update(k.iter().map(|b| b ^ 0x36).collect::<Vec<u8>>());
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.iter().map(|b| b ^ 0x5c).collect::<Vec<u8>>());
    outer.update(inner.finalize());
    outer.finalize().to_vec()
}

fn hmac_hex(key: &[u8], msg: &str) -> String {
    hex::encode(hmac_sha256(key, msg.as_bytes()))
}

/// Unix 秒 → UTC 的 (年月日 "YYYYMMDD", 时分秒 "HHMMSS")。
pub fn utc_parts(ts: u64) -> (String, String) {
    let days = (ts / 86400) as i64;
    let rem = ts % 86400;
    // 公历日期换算（Howard Hinnant 的 civil_from_days）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (format!("{y:04}{m:02}{d:02}"), format!("{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60))
}

/// RFC 3986 百分号编码（只放过 A-Z a-z 0-9 - _ . ~）。
pub fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 腾讯云 API 3.0（TC3-HMAC-SHA256）。返回要加的请求头（不含 Content-Type 以外的固定头由调用方设置）。
/// 签名的头：content-type、host、x-tc-action。
#[allow(clippy::too_many_arguments)]
pub fn tencent_headers(
    secret_id: &str,
    secret_key: &str,
    ts: u64,
    host: &str,
    service: &str,
    action: &str,
    version: &str,
    region: &str,
    payload: &str,
) -> Vec<(String, String)> {
    let (date, _) = utc_parts(ts);
    let ct = "application/json; charset=utf-8";
    let canonical = format!(
        "POST\n/\n\ncontent-type:{ct}\nhost:{host}\nx-tc-action:{}\n\ncontent-type;host;x-tc-action\n{}",
        action.to_lowercase(),
        sha256_hex(payload.as_bytes())
    );
    let scope = format!("{date}/{service}/tc3_request");
    let to_sign = format!("TC3-HMAC-SHA256\n{ts}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let k_date = hmac_sha256(format!("TC3{secret_key}").as_bytes(), date.as_bytes());
    let k_service = hmac_sha256(&k_date, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"tc3_request");
    let sig = hmac_hex(&k_signing, &to_sign);
    vec![
        ("Authorization".into(), format!("TC3-HMAC-SHA256 Credential={secret_id}/{scope}, SignedHeaders=content-type;host;x-tc-action, Signature={sig}")),
        ("Content-Type".into(), ct.into()),
        ("Host".into(), host.into()),
        ("X-TC-Action".into(), action.into()),
        ("X-TC-Timestamp".into(), ts.to_string()),
        ("X-TC-Version".into(), version.into()),
        ("X-TC-Region".into(), region.into()),
    ]
}

/// 阿里云 API 签名 V3（ACS3-HMAC-SHA256），参数放在查询串里、正文为空的 POST。
#[allow(clippy::too_many_arguments)]
pub fn aliyun_headers(
    key_id: &str,
    key_secret: &str,
    ts: u64,
    nonce: &str,
    host: &str,
    action: &str,
    version: &str,
    query: &[(&str, &str)],
) -> Vec<(String, String)> {
    let (date, time) = utc_parts(ts);
    let x_date = format!("{}-{}-{}T{}:{}:{}Z", &date[..4], &date[4..6], &date[6..], &time[..2], &time[2..4], &time[4..]);
    let body_hash = sha256_hex(b"");
    let mut q: Vec<(String, String)> = query.iter().map(|(k, v)| (pct(k), pct(v))).collect();
    q.sort();
    let cq = q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    let canonical = format!(
        "POST\n/\n{cq}\nhost:{host}\nx-acs-action:{action}\nx-acs-content-sha256:{body_hash}\nx-acs-date:{x_date}\nx-acs-signature-nonce:{nonce}\nx-acs-version:{version}\n\nhost;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version\n{body_hash}"
    );
    let to_sign = format!("ACS3-HMAC-SHA256\n{}", sha256_hex(canonical.as_bytes()));
    let sig = hmac_hex(key_secret.as_bytes(), &to_sign);
    vec![
        (
            "Authorization".into(),
            format!("ACS3-HMAC-SHA256 Credential={key_id},SignedHeaders=host;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version,Signature={sig}"),
        ),
        ("Host".into(), host.into()),
        ("x-acs-action".into(), action.into()),
        ("x-acs-content-sha256".into(), body_hash),
        ("x-acs-date".into(), x_date),
        ("x-acs-signature-nonce".into(), nonce.into()),
        ("x-acs-version".into(), version.into()),
    ]
}

/// 火山引擎 API 签名（HMAC-SHA256，和 AWS V4 同一类）。`query` 是已排好序、编码好的查询串。
#[allow(clippy::too_many_arguments)]
pub fn volc_headers(access_key: &str, secret_key: &str, ts: u64, host: &str, region: &str, service: &str, query: &str, payload: &str) -> Vec<(String, String)> {
    let (date, time) = utc_parts(ts);
    let x_date = format!("{date}T{time}Z");
    let body_hash = sha256_hex(payload.as_bytes());
    let canonical = format!(
        "POST\n/\n{query}\ncontent-type:application/json\nhost:{host}\nx-content-sha256:{body_hash}\nx-date:{x_date}\n\ncontent-type;host;x-content-sha256;x-date\n{body_hash}"
    );
    let scope = format!("{date}/{region}/{service}/request");
    let to_sign = format!("HMAC-SHA256\n{x_date}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let k_date = hmac_sha256(secret_key.as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"request");
    let sig = hmac_hex(&k_signing, &to_sign);
    vec![
        (
            "Authorization".into(),
            format!("HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders=content-type;host;x-content-sha256;x-date, Signature={sig}"),
        ),
        ("Content-Type".into(), "application/json".into()),
        ("Host".into(), host.into()),
        ("X-Content-Sha256".into(), body_hash),
        ("X-Date".into(), x_date),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h<'a>(v: &'a [(String, String)], k: &str) -> &'a str {
        v.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    #[test]
    fn hmac_matches_rfc4231_vectors() {
        // 测试用例 1
        assert_eq!(hex::encode(hmac_sha256(&[0x0b; 20], b"Hi There")), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
        // 测试用例 2
        assert_eq!(hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // 测试用例 6：密钥比分组长，要先哈希
        assert_eq!(
            hex::encode(hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn utc_time_parts() {
        assert_eq!(utc_parts(1_551_113_065), ("20190225".into(), "164425".into()));
        assert_eq!(utc_parts(1_700_000_000), ("20231114".into(), "221320".into()));
        assert_eq!(utc_parts(0), ("19700101".into(), "000000".into()));
        assert_eq!(utc_parts(951_782_400), ("20000229".into(), "000000".into()), "闰日");
    }

    #[test]
    fn percent_encoding_is_rfc3986() {
        assert_eq!(pct("a b&c=d/中~._-"), "a%20b%26c%3Dd%2F%E4%B8%AD~._-");
    }

    #[test]
    fn tencent_signature_has_the_documented_shape_and_is_deterministic() {
        let a =
            tencent_headers("AKID", "secret", 1_551_113_065, "tmt.tencentcloudapi.com", "tmt", "TextTranslateBatch", "2018-03-21", "ap-guangzhou", "{\"a\":1}");
        let b =
            tencent_headers("AKID", "secret", 1_551_113_065, "tmt.tencentcloudapi.com", "tmt", "TextTranslateBatch", "2018-03-21", "ap-guangzhou", "{\"a\":1}");
        assert_eq!(a, b);
        let auth = h(&a, "authorization");
        assert!(
            auth.starts_with("TC3-HMAC-SHA256 Credential=AKID/20190225/tmt/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature="),
            "{auth}"
        );
        assert_eq!(auth.rsplit("Signature=").next().unwrap().len(), 64);
        assert_eq!(h(&a, "x-tc-timestamp"), "1551113065");
        // 正文或密钥变了，签名就变
        let c =
            tencent_headers("AKID", "secret", 1_551_113_065, "tmt.tencentcloudapi.com", "tmt", "TextTranslateBatch", "2018-03-21", "ap-guangzhou", "{\"a\":2}");
        assert_ne!(h(&a, "authorization"), h(&c, "authorization"));
    }

    #[test]
    fn aliyun_signature_sorts_and_encodes_the_query() {
        let q = [("TargetLanguage", "zh"), ("SourceText", "Hello World"), ("FormatType", "text")];
        let a = aliyun_headers("LTAI", "sk", 1_700_000_000, "n1", "mt.cn-hangzhou.aliyuncs.com", "TranslateGeneral", "2018-10-12", &q);
        let mut r = q;
        r.reverse();
        let b = aliyun_headers("LTAI", "sk", 1_700_000_000, "n1", "mt.cn-hangzhou.aliyuncs.com", "TranslateGeneral", "2018-10-12", &r);
        assert_eq!(h(&a, "authorization"), h(&b, "authorization"), "参数顺序不影响签名");
        assert_eq!(h(&a, "x-acs-date"), "2023-11-14T22:13:20Z");
        assert_eq!(h(&a, "x-acs-content-sha256"), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(h(&a, "authorization").starts_with("ACS3-HMAC-SHA256 Credential=LTAI,SignedHeaders=host;x-acs-action;"));
        let c =
            aliyun_headers("LTAI", "sk", 1_700_000_000, "n1", "mt.cn-hangzhou.aliyuncs.com", "TranslateGeneral", "2018-10-12", &[("SourceText", "Hello Moon")]);
        assert_ne!(h(&a, "authorization"), h(&c, "authorization"));
    }

    #[test]
    fn volcengine_signature_scope_and_headers() {
        let a = volc_headers(
            "AK",
            "SK",
            1_700_000_000,
            "translate.volcengineapi.com",
            "cn-north-1",
            "translate",
            "Action=TranslateText&Version=2020-06-01",
            "{\"TargetLanguage\":\"zh\"}",
        );
        let auth = h(&a, "authorization");
        assert!(
            auth.starts_with(
                "HMAC-SHA256 Credential=AK/20231114/cn-north-1/translate/request, SignedHeaders=content-type;host;x-content-sha256;x-date, Signature="
            ),
            "{auth}"
        );
        assert_eq!(h(&a, "x-date"), "20231114T221320Z");
        assert_eq!(h(&a, "x-content-sha256"), sha256_hex(b"{\"TargetLanguage\":\"zh\"}"));
    }
}
