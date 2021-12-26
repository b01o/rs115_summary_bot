use anyhow::Result;
use lazy_static::lazy_static;
use openssl::symm::{decrypt, Cipher};
const KEY: &[u8; 16] = b"zhshimima1112221";
const COMMON_SUFFIX: &str = "42IcwVjnnGHZB9ehzW+Pew==";
lazy_static! {
    static ref CIPHER: Cipher = Cipher::aes_128_ecb();
}
pub fn preid_decrypt(data: &str) -> Result<String> {
    let content = base64::decode(data.to_owned() + COMMON_SUFFIX)?;
    let res = decrypt(*CIPHER, KEY, None, &content)?;
    Ok(std::str::from_utf8(&res)?
        .trim_end_matches('\x00')
        .to_string())
}
