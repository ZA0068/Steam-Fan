//! Device pairing. One user, a few PCs: each PC is paired once with a 6-digit code
//! and gets its own token. The server keeps only a hash of each token in
//! `devices.json`, so a device can be revoked without touching the others.
//!
//! Get a code:  `docker exec steam-fan-server steam-fan-server pair`
//! (one is also printed in the log when the server starts and nothing is paired yet).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const CODE_MINUTES: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub hash: String,
    pub paired: u64,
    #[serde(default)]
    pub last_seen: u64,
}

#[derive(Serialize, Deserialize)]
struct Code {
    code: String,
    expires: u64,
    #[serde(default)]
    tries: u32,
}

pub struct Auth {
    dir: PathBuf,
    /// A fixed token from the SF_TOKEN variable, for scripts. Empty: none.
    fixed: String,
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    if fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        // never expected on Linux; still not predictable across runs
        let seed = format!("{:?}{}", std::time::SystemTime::now(), std::process::id());
        b = Sha256::digest(seed.as_bytes())[..n.min(32)].to_vec();
    }
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn write_private(p: &Path, text: &str) -> Result<()> {
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(tmp, p)?;
    Ok(())
}

impl Auth {
    pub fn new(dir: &Path) -> Auth {
        let _ = fs::create_dir_all(dir);
        Auth { dir: dir.to_path_buf(), fixed: std::env::var("SF_TOKEN").unwrap_or_default().trim().to_string() }
    }

    fn devices_path(&self) -> PathBuf {
        self.dir.join("devices.json")
    }
    fn code_path(&self) -> PathBuf {
        self.dir.join("pair-code.json")
    }

    pub fn devices(&self) -> Vec<Device> {
        fs::read_to_string(self.devices_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self, d: &[Device]) -> Result<()> {
        write_private(&self.devices_path(), &serde_json::to_string_pretty(d)?)
    }

    /// A fresh 6-digit pairing code, valid for 10 minutes.
    pub fn new_code(&self) -> Result<String> {
        let b = random_bytes(4);
        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) % 1_000_000;
        let code = format!("{n:06}");
        let c = Code { code: code.clone(), expires: crate::jobs::now() + CODE_MINUTES * 60, tries: 0 };
        write_private(&self.code_path(), &serde_json::to_string(&c)?)?;
        Ok(code)
    }

    /// Trade a pairing code for a token. The code works once.
    pub fn pair(&self, code: &str, name: &str) -> Result<String> {
        let mut c: Code = fs::read_to_string(self.code_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .context("no pairing code is waiting: run `steam-fan-server pair` on the NAS")?;
        if c.expires < crate::jobs::now() {
            let _ = fs::remove_file(self.code_path());
            bail!("the pairing code expired: run `steam-fan-server pair` again");
        }
        if c.code != code.trim() {
            // a few tries only, so the code can't be guessed
            c.tries += 1;
            if c.tries >= 5 {
                let _ = fs::remove_file(self.code_path());
                bail!("wrong pairing code, 5 times: run `steam-fan-server pair` for a new one");
            }
            write_private(&self.code_path(), &serde_json::to_string(&c)?)?;
            bail!("wrong pairing code");
        }
        let _ = fs::remove_file(self.code_path());
        let name = if name.trim().is_empty() { "PC".to_string() } else { name.trim().to_string() };
        let token = hex(&random_bytes(32));
        let mut d = self.devices();
        d.retain(|x| x.name != name); // pairing again replaces the old token
        d.push(Device { name, hash: hash(&token), paired: crate::jobs::now(), last_seen: crate::jobs::now() });
        self.save(&d)?;
        Ok(token)
    }

    /// Which device a token belongs to.
    pub fn check(&self, token: &str) -> Option<String> {
        let token = token.trim();
        if token.is_empty() {
            return None;
        }
        if !self.fixed.is_empty() && token == self.fixed {
            return Some("SF_TOKEN".into());
        }
        let h = hash(token);
        let mut d = self.devices();
        let i = d.iter().position(|x| x.hash == h)?;
        let name = d[i].name.clone();
        let now = crate::jobs::now();
        if now.saturating_sub(d[i].last_seen) > 3600 {
            d[i].last_seen = now;
            let _ = self.save(&d);
        }
        Some(name)
    }

    pub fn revoke(&self, name: &str) -> Result<bool> {
        let mut d = self.devices();
        let before = d.len();
        d.retain(|x| x.name != name);
        self.save(&d)?;
        Ok(d.len() != before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pair_check_revoke() {
        let t = tempfile::tempdir().unwrap();
        let a = Auth::new(t.path());
        assert!(a.pair("123456", "pc").is_err());
        let code = a.new_code().unwrap();
        assert_eq!(code.len(), 6);
        assert!(a.pair("000000x", "pc").is_err());
        for _ in 0..3 {
            assert!(a.pair("x", "pc").is_err());
        }
        let tok = a.pair(&code, "zain-pc").unwrap();
        assert!(a.pair(&code, "again").is_err(), "a code works once");
        assert_eq!(a.check(&tok).as_deref(), Some("zain-pc"));
        assert!(a.check("nope").is_none());
        assert!(!fs::read_to_string(t.path().join("devices.json")).unwrap().contains(&tok), "only the hash is stored");
        assert!(a.revoke("zain-pc").unwrap());
        assert!(a.check(&tok).is_none());
    }
}
