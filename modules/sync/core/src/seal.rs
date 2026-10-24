//! Secrets in a sync file, such as the dandanplay token and the Bangumi
//! session, are encrypted with a key drawn from the sync passphrase the
//! user chose; the passphrase itself never leaves the device.
//!
//! - The key: PBKDF2-HMAC-SHA256 of the passphrase with a random salt kept
//!   beside it in the file (the repository's on WebDAV), 64 bytes: half
//!   for AES-256, half for HMAC-SHA256. A check value of the key, also
//!   kept beside it, tells a wrong passphrase from damage.
//! - A secret: deterministic authenticated encryption in the SIV manner.
//!   The tag is HMAC-SHA256 over the secret's purpose and its text, cut to
//!   16 bytes; it is also the AES-256-CTR counter the text is encrypted
//!   with. The same secret under the same key encrypts alike, so syncing
//!   sees no change where there is none; all it gives away is whether two
//!   sealed secrets are equal.

use aes::Aes256;
use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// PBKDF2 rounds of new files: slow to guess, about a second on a phone.
pub const ROUNDS: u32 = 200_000;
/// What the check value is the MAC of.
const CHECK: &[u8] = b"erika-sync-check-v1";
/// The first byte of a sealed secret: how it was sealed.
const SEALED_V1: u8 = 1;

/// How a key is drawn from the passphrase, as a file keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyParams {
    pub salt: Vec<u8>,
    pub rounds: u32,
    pub check: Vec<u8>,
}

#[derive(Clone)]
pub struct SealKey {
    encrypt: [u8; 32],
    mac: [u8; 32],
    params: KeyParams,
}

impl std::fmt::Debug for SealKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SealKey(…)")
    }
}

impl SealKey {
    /// The key of `passphrase` with a new salt.
    pub fn create(passphrase: &str, rounds: u32) -> SealKey {
        Self::derive(passphrase, &crate::random_bytes(16), rounds)
    }

    /// The key of `passphrase` as `params` says, when it is the key they
    /// were made with; `None` for another passphrase.
    pub fn open(passphrase: &str, params: &KeyParams) -> Option<SealKey> {
        let key = Self::derive(passphrase, &params.salt, params.rounds);
        let mut mac = key.mac();
        mac.update(CHECK);
        mac.verify_truncated_left(&params.check).ok()?;
        Some(key)
    }

    fn derive(passphrase: &str, salt: &[u8], rounds: u32) -> SealKey {
        let mut out = [0u8; 64];
        pbkdf2(passphrase.as_bytes(), salt, rounds.max(1), &mut out);
        let mut key = SealKey {
            encrypt: out[..32].try_into().expect("32 bytes"),
            mac: out[32..].try_into().expect("32 bytes"),
            params: KeyParams {
                salt: salt.to_vec(),
                rounds: rounds.max(1),
                check: Vec::new(),
            },
        };
        let mut mac = key.mac();
        mac.update(CHECK);
        key.params.check = mac.finalize().into_bytes()[..16].to_vec();
        key
    }

    pub fn params(&self) -> &KeyParams {
        &self.params
    }

    fn mac(&self) -> HmacSha256 {
        <HmacSha256 as Mac>::new_from_slice(&self.mac).expect("any key length")
    }

    fn tag(&self, purpose: &str, text: &[u8]) -> [u8; 16] {
        let mut mac = self.mac();
        mac.update(purpose.as_bytes());
        mac.update(&[0]);
        mac.update(text);
        mac.finalize().into_bytes()[..16]
            .try_into()
            .expect("16 bytes")
    }

    /// `text` encrypted for `purpose`; only the same purpose opens it.
    pub fn seal(&self, purpose: &str, text: &[u8]) -> Vec<u8> {
        let tag = self.tag(purpose, text);
        let mut out = Vec::with_capacity(1 + 16 + text.len());
        out.push(SEALED_V1);
        out.extend_from_slice(&tag);
        let start = out.len();
        out.extend_from_slice(text);
        self.ctr(&tag, &mut out[start..]);
        out
    }

    /// What [`seal`](Self::seal) sealed for `purpose`, when it was this key
    /// and nothing changed it.
    pub fn open_sealed(&self, purpose: &str, sealed: &[u8]) -> Option<Vec<u8>> {
        let (&version, rest) = sealed.split_first()?;
        if version != SEALED_V1 || rest.len() < 16 {
            return None;
        }
        let (tag, cipher) = rest.split_at(16);
        let mut text = cipher.to_vec();
        self.ctr(tag.try_into().expect("16 bytes"), &mut text);
        let mut mac = self.mac();
        mac.update(purpose.as_bytes());
        mac.update(&[0]);
        mac.update(&text);
        mac.verify_truncated_left(tag).ok()?;
        Some(text)
    }

    /// AES-256 in counter mode from `counter`, a 128-bit big-endian number.
    fn ctr(&self, counter: &[u8; 16], data: &mut [u8]) {
        let cipher = Aes256::new(GenericArray::from_slice(&self.encrypt));
        let mut count = u128::from_be_bytes(*counter);
        for chunk in data.chunks_mut(16) {
            let mut block = GenericArray::clone_from_slice(&count.to_be_bytes());
            cipher.encrypt_block(&mut block);
            for (byte, pad) in chunk.iter_mut().zip(block.iter()) {
                *byte ^= pad;
            }
            count = count.wrapping_add(1);
        }
    }
}

/// PBKDF2 with HMAC-SHA256 (RFC 8018).
pub fn pbkdf2(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
    let keyed = <HmacSha256 as Mac>::new_from_slice(password).expect("any key length");
    for (index, chunk) in out.chunks_mut(32).enumerate() {
        let mut mac = keyed.clone();
        mac.update(salt);
        mac.update(&(index as u32 + 1).to_be_bytes());
        let mut u = mac.finalize().into_bytes();
        let mut t = u;
        for _ in 1..rounds {
            let mut mac = keyed.clone();
            mac.update(&u);
            u = mac.finalize().into_bytes();
            for (t, u) in t.iter_mut().zip(u.iter()) {
                *t ^= u;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}
