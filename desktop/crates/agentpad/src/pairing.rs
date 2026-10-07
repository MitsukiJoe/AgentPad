use hmac::{Hmac, Mac};
use sha2::Sha256;

const MAX_CODE_FAILURES: u8 = 5;

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS random source");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn mac(secret: &str, nonce: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(nonce.as_bytes());
    mac
}

#[cfg(test)]
pub fn auth_tag(secret: &str, nonce: &str) -> String {
    mac(secret, nonce)
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `tag` 为小写/大写十六进制 HMAC-SHA256(secret, nonce)；常量时间比较。
pub fn verify_auth(secret: &str, nonce: &str, tag: &str) -> bool {
    let Some(bytes) = decode_hex(tag) else {
        return false;
    };
    !secret.is_empty() && mac(secret, nonce).verify_slice(&bytes).is_ok()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() != 64 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// 手动输入 IP 时的一次性 4 位配对码：仅配对窗可见时有效，成功即换新，累计错 5 次锁定直到用户换码。
#[derive(Default)]
pub struct Pairing {
    window_open: bool,
    code: Option<String>,
    failures: u8,
}

impl Pairing {
    pub fn set_window_open(&mut self, open: bool) {
        if open && !self.window_open {
            self.renew();
        } else if !open {
            self.code = None;
        }
        self.window_open = open;
    }

    pub fn renew(&mut self) {
        self.code = Some(random_code());
        self.failures = 0;
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn try_code(&mut self, attempt: &str) -> bool {
        let Some(code) = &self.code else {
            return false;
        };
        let ok = code.len() == attempt.len()
            && code
                .bytes()
                .zip(attempt.bytes())
                .fold(0, |acc, (a, b)| acc | (a ^ b))
                == 0;
        if ok {
            self.renew();
        } else {
            self.failures += 1;
            if self.failures >= MAX_CODE_FAILURES {
                self.code = None;
            }
        }
        ok
    }
}

fn random_code() -> String {
    loop {
        let mut b = [0u8; 2];
        getrandom::fill(&mut b).expect("OS random source");
        let n = u16::from_le_bytes(b);
        // 60000 以下取模无偏。
        if n < 60_000 {
            return format!("{:04}", n % 10_000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc4231_case_2() {
        assert_eq!(
            auth_tag("Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        let nonce = random_hex(16);
        assert_eq!(nonce.len(), 32);
        let secret = random_hex(32);
        let tag = auth_tag(&secret, &nonce);
        assert!(verify_auth(&secret, &nonce, &tag));
        assert!(verify_auth(&secret, &nonce, &tag.to_uppercase()));
        assert!(!verify_auth(&secret, &random_hex(16), &tag));
        assert!(!verify_auth(&random_hex(32), &nonce, &tag));
        assert!(!verify_auth("", &nonce, &auth_tag("", &nonce)));
        assert!(!verify_auth(&secret, &nonce, "zz"));
        assert!(!verify_auth(&secret, &nonce, &tag[..62]));
    }

    #[test]
    fn hmac_decoder_rejects_non_sha256_lengths() {
        for len in [66, 1 << 20, 0, 2, 62, 63, 65] {
            assert!(decode_hex(&"a".repeat(len)).is_none(), "length {len}");
        }
    }

    #[test]
    fn code_is_single_use_window_bound_and_locks_after_five_failures() {
        let mut p = Pairing::default();
        assert_eq!(p.code(), None);
        assert!(!p.try_code("0000"));
        p.set_window_open(true);
        let code = p.code().unwrap().to_string();
        assert_eq!(code.len(), 4);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
        p.set_window_open(true);
        assert_eq!(p.code(), Some(code.as_str()));
        assert!(p.try_code(&code));
        let code = p.code().unwrap().to_string();

        let wrong = if code == "0000" { "0001" } else { "0000" };
        for _ in 0..4 {
            assert!(!p.try_code(wrong));
        }
        assert_eq!(p.code(), Some(code.as_str()));
        assert!(!p.try_code(wrong));
        assert_eq!(p.code(), None);
        assert!(!p.try_code(&code));
        p.renew();
        assert!(p.code().is_some());

        p.set_window_open(false);
        assert_eq!(p.code(), None);
        p.set_window_open(true);
        assert!(p.code().is_some());
    }
}
