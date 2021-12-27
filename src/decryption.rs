use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use openssl::symm::{decrypt, Cipher};
use regex::Regex;
const KEY: &[u8; 16] = b"zhshimima1112221";
const COMMON_SUFFIX: &str = "42IcwVjnnGHZB9ehzW+Pew==";
lazy_static! {
    static ref CIPHER: Cipher = Cipher::aes_128_ecb();
    static ref PATH_ID_REGEX: Regex = Regex::new(r":\d*?/").unwrap();
}
pub fn preid_decrypt(data: &str) -> Result<String> {
    let content = base64::decode(data.to_owned() + COMMON_SUFFIX)?;
    let res = decrypt(*CIPHER, KEY, None, &content)?;
    Ok(std::str::from_utf8(&res)?
        .trim_end_matches('\x00')
        .to_string())
}
pub fn format_path_str(path_str: &str) -> Result<String> {
    let path_str = path_str
        .replace(" ", "_")
        .replace("\\", "")
        .replace("\n", "");

    let path_str = PATH_ID_REGEX.replace_all(&path_str, "|");
    let path_str = path_str
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("wrong db path_str format..."))?
        .0;
    Ok(path_str.to_string())
}
