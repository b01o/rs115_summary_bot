use anyhow::{anyhow, Result};
use anyhow::{bail, Context};
use crypto::md5::Md5;
use crypto::{digest::Digest, sha1::Sha1};
use data_encoding::{BASE32_NOPAD, HEXUPPER};
use lazy_static::lazy_static;
use magnet_url::Magnet;
use pakr_iec::iec;
use regex::Regex;
use scopeguard::defer;
use serde::{Deserialize, Deserializer, Serialize};
use serde_bytes::ByteBuf;
use std::collections::HashSet;
use std::fmt;

use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

pub async fn json2line(input: &Path, output: &Path) -> Result<()> {
    check_input_output(input, output).await?;

    let mut file = open_without_bom(input).await?;
    let mut buf: Vec<u8> = Vec::new();
    file.read_to_end(&mut buf).await?;
    file.flush().await?;
    let entity: Sha1Entity = serde_json::from_slice(&buf)?;
    let file = std::fs::File::create(output)?;
    let mut writer = std::io::BufWriter::new(file);
    write_line(&mut writer, &entity, "".to_owned());
    Ok(())
}

pub fn json2line_mem(entity: &Sha1Entity) -> Result<String> {
    let mut res = String::new();
    write_line_mem(&mut res, entity, "".to_owned());
    Ok(res)
}

pub async fn line_strip_dir_info(input: &Path, output: &Path) -> Result<()> {
    check_input_output(input, output).await?;

    let file = open_without_bom(input).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let out_file = TokioFile::create(output).await.context(format!(
        "failed to create the output file:{}",
        output.to_string_lossy(),
    ))?;

    let mut writer = BufWriter::new(out_file);

    while let Some(line) = lines.next_line().await.context(format!(
        "failed to read line from file: {}",
        input.to_string_lossy()
    ))? {
        let repr = match FileRepr::from_str(&line) {
            Ok(file) => file,
            Err(_) => {
                log::warn!("invalid line during stripping dir info: {}", line);
                continue;
            }
        };
        let line = repr.to_sha1_link() + "\n";
        writer.write_all(line.as_bytes()).await?;
    }
    writer.flush().await?;

    Ok(())
}

pub async fn is_valid_line(input: &Path) -> Result<()> {
    check_input(input).await?;

    let file = open_without_bom(input).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    if let Some(line) = lines.next_line().await.context(format!(
        "failed to read line from file: {}",
        input.to_string_lossy()
    ))? {
        if line.len() > 3000 || line.starts_with('{') || line.starts_with('[') {
            return Err(WrongSha1LinkFormat.into());
        }

        if line.split('|').count() < 4 {
            return Err(WrongSha1LinkFormat.into());
        }
        return Ok(());
    }

    Err(WrongSha1LinkFormat.into())
}

pub async fn line2json(input: &Path, output: &Path) -> Result<()> {
    check_input_output(input, output).await?;

    let mut root_list: Vec<Sha1Entity> = Vec::new();
    let file = open_without_bom(input).await?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await.context(format!(
        "failed to read line from file: {}",
        input.to_string_lossy()
    ))? {
        let parts: Vec<&str> = line.split('|').collect();

        let repr = match FileRepr::from_str(&line) {
            Ok(file) => file,
            Err(_) => {
                log::warn!("invalid line during stripping dir info: {}", line);
                continue;
            }
        };

        let mut curr_list: &mut Vec<_> = &mut root_list;
        for part in parts.iter().take(parts.len() - 1).skip(4) {
            let folder = get_dir_or_create(part, curr_list);
            curr_list = &mut folder.dirs;
        }
        let folder = get_dir_or_create(parts[parts.len() - 1], curr_list);
        folder.files.push(repr);
    }

    // if list contains multiple entities, create a parent folder for them

    let out_file = TokioFile::create(output).await.context(format!(
        "failed to create the output file:{}",
        output.to_string_lossy(),
    ))?;

    let mut writer = BufWriter::new(out_file);
    let result: Vec<u8>;
    if root_list.len() > 1 {
        let mut new_parent = Sha1Entity::new("new_folder".to_owned());
        new_parent.dirs = root_list;
        result = serde_json::to_vec(&new_parent)?;
    } else {
        result = serde_json::to_vec(&root_list[0])?;
    }
    writer.write_all(&result).await?;
    writer.flush().await?;

    Ok(())
}

pub async fn path_to_sha1_entity(input: &Path) -> Result<Sha1Entity> {
    check_input(input).await?;

    let mut json_file = open_without_bom(input).await?;

    let mut json: Vec<u8> = Vec::new();
    json_file.read_to_end(&mut json).await?;
    json_file.flush().await?;

    let sha1: Sha1Entity = serde_json::from_slice(&json)?;

    Ok(sha1)
}

pub async fn check_dup_n_err(path: &Path) -> Result<(usize, usize)> {
    check_input(path).await?;
    let file = open_without_bom(path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut invalid_lines: usize = 0;
    let mut list: Vec<FileRepr> = Vec::new();

    while let Some(line) = lines
        .next_line()
        .await
        .context("fail to read line, maybe not utf8?")?
    {
        if line.is_empty() || line.chars().all(|c| c.is_ascii_whitespace()) {
            continue;
        }

        let repr = match FileRepr::from_str(&line) {
            Ok(file) => file,
            Err(_) => {
                log::warn!("invalid line during check dup_n_err : {}", line);
                invalid_lines += 1;
                continue;
            }
        };

        list.push(repr);
    }

    let origin = list.len();

    if origin == 0 {
        return Err(anyhow!(
            "{:?}: file does not contain sha1 link, fail to find dup",
            path
        ));
    }

    let list = dedup_filerepr_vec(list);
    let after = list.len();

    Ok((origin - after, invalid_lines))
}

pub async fn dedup_filerepr_file(input: &Path, output: &Path) -> Result<()> {
    check_input_output(input, output).await?;

    let file = open_without_bom(input).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut set = HashSet::new();

    let output = TokioFile::create(output).await?;
    let mut writer = BufWriter::new(output);

    while let Some(line) = lines
        .next_line()
        .await
        .context("fail to read line, maybe not utf8?")?
    {
        if line.is_empty() || line.chars().all(|c| c.is_ascii_whitespace()) {
            continue;
        }

        let repr = match FileRepr::from_str(&line) {
            Ok(file) => file,
            Err(_) => {
                log::warn!("invalid line during dedup file {:?} info: {}", input, line,);
                continue;
            }
        };

        if !set.contains(&repr.unique_key()) {
            set.insert(repr.unique_key());
            writer.write_all(format!("{}\n", line).as_bytes()).await?;
        }
    }

    writer.flush().await?;

    Ok(())
}

pub fn dedup_filerepr_vec(mut list: Vec<FileRepr>) -> Vec<FileRepr> {
    let mut set = HashSet::new();
    list.retain(|item| {
        if set.contains(&item.unique_key()) {
            false
        } else {
            set.insert(item.unique_key());
            true
        }
    });

    list
}

pub fn line_summary_mem(content: &str) -> Result<Summary> {
    let mut all_size: Vec<u64> = Vec::new();
    let mut num_lines: u64 = 0;
    let mut has_folder = true;

    for line in content.lines() {
        if line.is_empty() || line.chars().all(|c| c.is_ascii_whitespace()) {
            continue;
        }
        num_lines += 1;
        let mut parts = line.split('|');
        // log::info!("{}", line);
        let size: u64 = parts
            .nth(1)
            .ok_or_else(|| anyhow!("wrong format"))?
            .parse()?;
        all_size.push(size);
        if parts.nth(2).is_none() {
            has_folder = false;
        }
    }
    if num_lines == 0 {
        bail!("empty lines!");
    }

    all_size.sort_unstable();
    let max: u64 = all_size[all_size.len() - 1];
    let min = all_size[0];
    let total_size = all_size.iter().sum();

    let mid: f64 = if all_size.len() % 2 == 1 {
        all_size[all_size.len() / 2] as f64
    } else {
        let left = all_size[all_size.len() / 2 - 1];
        let right = all_size[all_size.len() / 2];
        (left + right) as f64 / 2.0
    };

    Ok(Summary {
        total_size,
        max,
        min,
        mid,
        total_files: num_lines,
        has_folder,
    })
}

pub async fn line_summary(path: &Path) -> Result<Summary> {
    check_input(path).await?;

    let mut all_size: Vec<u64> = Vec::new();
    let mut num_lines: u64 = 0;
    let mut has_folder = true;

    let file = open_without_bom(path).await?;
    let reader = tokio::io::BufReader::new(file);
    let mut lines = reader.lines();
    while let Some(line) = lines
        .next_line()
        .await
        .context("fail to read line, maybe not utf8?")?
    {
        if line.is_empty() || line.chars().all(|c| c.is_ascii_whitespace()) {
            continue;
        }
        // log::info!("{}", line);
        num_lines += 1;
        let mut parts = line.split('|');
        let size: u64 = parts
            .nth(1)
            .ok_or_else(|| anyhow!("wrong format"))?
            .parse()?;
        all_size.push(size);
        if parts.nth(2).is_none() {
            has_folder = false;
        }
    }

    if num_lines == 0 {
        bail!("failed to read line of file:{}", path.to_string_lossy());
    }
    all_size.sort_unstable();
    let max: u64 = all_size[all_size.len() - 1];
    let min = all_size[0];
    let total_size = all_size.iter().sum();

    let mid: f64 = if all_size.len() % 2 == 1 {
        all_size[all_size.len() / 2] as f64
    } else {
        let left = all_size[all_size.len() / 2 - 1];
        let right = all_size[all_size.len() / 2];
        (left as f64 + right as f64) / 2.0
    };

    Ok(Summary {
        total_size,
        max,
        min,
        mid,
        total_files: num_lines,
        has_folder,
    })
}

pub fn json_summary(entity: &Sha1Entity) -> Result<Summary> {
    let lines = json2line_mem(entity)?;
    let mut res = line_summary_mem(&lines)?;
    res.has_folder = true;
    Ok(res)
}

#[derive(Debug)]
pub struct Summary {
    total_size: u64,
    max: u64,
    min: u64,
    mid: f64,
    total_files: u64,
    pub has_folder: bool,
}

pub fn to_iec(num: impl Into<u128>) -> String {
    let mut res = iec(num.into());
    if res.ends_with('i') {
        res.pop();
    } else {
        res.push('B')
    }
    res
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "共 {} 个文件, {}\n最小 {}, 最大 {}, 中位 {}",
            self.total_files,
            to_iec(self.total_size),
            to_iec(self.min),
            to_iec(self.max),
            to_iec(self.mid as u128),
        )
    }
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct Sha1Entity {
    #[serde(deserialize_with = "from_dirty_string")]
    dir_name: String,
    files: Vec<FileRepr>,
    dirs: Vec<Self>,
    #[serde(skip_serializing)]
    id: Option<u64>,
}

fn from_dirty_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let mut s: String = Deserialize::deserialize(deserializer)?;
    s.retain(|c| c != '\r' && c != '\n' && c != '\\');
    Ok(s)
}

impl Sha1Entity {
    pub fn new(dir_name: String) -> Self {
        Self {
            dir_name,
            files: Vec::new(),
            dirs: Vec::new(),
            id: None,
        }
    }
}

impl FromStr for Sha1Entity {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path = Path::new(s);
        if !path.exists() {
            return Err(anyhow!("File not exist."));
        }
        let file = std::fs::File::open(s)?;
        let mut reader = std::io::BufReader::new(file);
        if has_bom(path)? {
            reader.seek_relative(3)?;
        }
        Ok(serde_json::from_reader::<_, Sha1Entity>(reader)?)
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct FileRepr {
    // #[serde(deserialize_with = "from_dirty_string")]
    name: String,
    size: u64,
    sha1: String,
    sha1_block: String,
    id: Option<u64>,
}

impl FromStr for FileRepr {
    type Err = WrongSha1LinkFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('|');
        let mut name = parts.next().ok_or(WrongSha1LinkFormat)?.to_string();
        if name.starts_with("115://") {
            name = name.strip_prefix("115://").unwrap().to_string();
        }
        let size = parts
            .next()
            .ok_or(WrongSha1LinkFormat)?
            .parse()
            .map_err(|_| WrongSha1LinkFormat)?;

        let sha1 = parts.next().ok_or(WrongSha1LinkFormat)?.to_string();
        let sha1_block = parts.next().ok_or(WrongSha1LinkFormat)?.to_string();

        if sha1.len() != 40 || sha1_block.len() != 40 {
            return Err(WrongSha1LinkFormat);
        }
        if !(sha1.chars().all(|c| c.is_digit(16)) && sha1_block.chars().all(|c| c.is_digit(16))) {
            return Err(WrongSha1LinkFormat);
        }

        Ok(FileRepr {
            name,
            size,
            sha1,
            sha1_block,
            id: None,
        })
    }
}

impl FileRepr {
    fn to_sha1_link(&self) -> String {
        "115://".to_owned()
            + &[
                self.name.to_owned(),
                self.size.to_string(),
                self.sha1.to_owned(),
                self.sha1_block.to_owned(),
            ]
            .join("|")
    }

    fn unique_key(&self) -> String {
        format!("{}{}{}", self.size, self.sha1, self.sha1_block)
    }
}

impl Serialize for FileRepr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut out = [
            self.name.to_owned(),
            self.size.to_string(),
            self.sha1.to_owned(),
            self.sha1_block.to_owned(),
        ]
        .join("|");
        if out.starts_with("115://") {
            out = out.strip_prefix("115://").unwrap().to_owned();
        }
        serializer.serialize_str(&out)
    }
}

#[derive(Debug)]
pub struct WrongSha1LinkFormat;

impl std::fmt::Display for WrongSha1LinkFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wrong format of 115sha1link")
    }
}

impl std::error::Error for WrongSha1LinkFormat {}

use serde::de::Error;

use crate::global::ROOT_FOLDER;
use crate::io::{check_input, check_input_output, has_bom, open_without_bom};
impl<'de> Deserialize<'de> for FileRepr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // 115://jfks|size|sha1|sha1_block

        let mut sp = s.split('|');

        let mut name = sp
            .next()
            .ok_or(WrongSha1LinkFormat)
            .map_err(D::Error::custom)?
            .to_owned();
        if name.starts_with("115://") {
            name = name.strip_prefix("115://").unwrap().to_owned();
        }
        name.retain(|c| c != '\n' && c != '\r');

        let size = sp
            .next()
            .ok_or(WrongSha1LinkFormat)
            .map_err(D::Error::custom)?
            .to_owned()
            .parse()
            .map_err(D::Error::custom)?;
        let sha1 = sp
            .next()
            .ok_or(WrongSha1LinkFormat)
            .map_err(D::Error::custom)?
            .to_owned();
        let sha1_block = sp
            .next()
            .ok_or(WrongSha1LinkFormat)
            .map_err(D::Error::custom)?
            .to_owned();

        Ok(Self {
            name,
            size,
            sha1,
            sha1_block,
            id: None,
        })
    }
}

#[derive(Debug)]
struct Parse115SHA1Error();
impl std::error::Error for Parse115SHA1Error {}

impl std::fmt::Display for Parse115SHA1Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid file")
    }
}

fn get_dir_or_create<'q>(name: &'_ str, queue: &'q mut Vec<Sha1Entity>) -> &'q mut Sha1Entity {
    let index = queue
        .iter()
        .enumerate()
        .find(|(_, p)| p.dir_name == name)
        .map(|(i, _)| i)
        .unwrap_or_else(|| {
            queue.push(Sha1Entity::new(name.to_owned()));
            queue.len() - 1
        });

    unsafe { queue.get_unchecked_mut(index) }
}

fn write_line_mem(res: &mut String, entity: &Sha1Entity, suffix: String) {
    let suffix = suffix + "|" + &entity.dir_name;
    for file in &entity.files {
        let link = file.to_sha1_link();
        let line = link + &suffix;
        let line = line + "\n";
        res.push_str(&line);
    }
    for dir in &entity.dirs {
        write_line_mem(res, dir, suffix.to_owned());
    }
}

fn write_line(writer: &mut std::io::BufWriter<std::fs::File>, entity: &Sha1Entity, suffix: String) {
    use std::io::Write;
    let suffix = suffix + "|" + &entity.dir_name;
    for file in &entity.files {
        let link = file.to_sha1_link();
        let line = link + &suffix;
        writeln!(writer, "{}", line).unwrap();
    }
    for dir in &entity.dirs {
        write_line(writer, dir, suffix.to_owned());
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Node(String, i64);

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
struct File {
    path: Vec<String>,
    length: i64,
    #[serde(default)]
    md5sum: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct Info {
    name: String,
    pieces: ByteBuf,
    #[serde(rename = "piece length")]
    piece_length: i64,
    #[serde(default)]
    md5sum: Option<String>,
    #[serde(default)]
    length: Option<i64>,
    #[serde(default)]
    files: Option<Vec<File>>,
    #[serde(default)]
    private: Option<u8>,
    #[serde(default)]
    path: Option<Vec<String>>,
    #[serde(default)]
    #[serde(rename = "root hash")]
    root_hash: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub struct Torrent {
    pub info: Info,
    #[serde(default, skip)]
    announce: Option<String>,
    #[serde(default, skip)]
    nodes: Option<Vec<Node>>,
    #[serde(default, skip)]
    encoding: Option<String>,
    #[serde(default, skip)]
    httpseeds: Option<Vec<String>>,
    #[serde(default, skip)]
    #[serde(rename = "announce-list")]
    announce_list: Option<Vec<Vec<String>>>,
    #[serde(default, skip)]
    #[serde(rename = "creation date")]
    creation_date: Option<i64>,
    #[serde(skip)]
    #[serde(rename = "comment")]
    comment: Option<String>,
    #[serde(default, skip)]
    #[serde(rename = "created by")]
    created_by: Option<String>,
}

#[allow(dead_code)]
fn sha1(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.input(bytes);
    hasher.result_str()
}

#[allow(dead_code)]
fn md5(bytes: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.input(bytes);
    hasher.result_str()
}

#[allow(dead_code)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub async fn get_torrent_magnet_async(path: &Path) -> Result<String> {
    check_input(path).await?;

    let output = Command::new("transmission-show")
        .arg("-m")
        .arg(path.as_os_str())
        .output()
        .context("transmission-show lanuch failed")?;

    if !output.status.success() {
        bail!("transsmission return err");
    }

    let magnet = Magnet::new(std::str::from_utf8(&output.stdout)?)
        .map_err(|_| anyhow!("magnet parse fail"))?;
    let xt = magnet.xt.ok_or_else(|| anyhow!("magnet parse fail"))?;
    Ok("magnet:?xt=urn:btih:".to_string() + &xt)
}

pub async fn get_torrent_name_async(path: &Path) -> Result<String> {
    check_input(path).await?;

    let get_name = Command::new("transmission-show")
        .arg(path.as_os_str())
        .output()
        .context("transmission-show lanuch failed")?;

    if !get_name.status.success() {
        bail!("transmission-show return err");
    }

    let mut name = std::str::from_utf8(&get_name.stdout)?
        .lines()
        .next()
        .ok_or_else(|| anyhow!("transmission-show stdout empty"))?;

    name = if name.trim_start().starts_with("Name:") {
        name.trim_start()
            .strip_prefix("Name:")
            .unwrap()
            .trim_start()
    } else {
        "Unknown"
    };

    Ok(name.to_string())
}

pub async fn get_torrent_summary_async(path: &Path) -> Result<String> {
    check_input(path).await?;

    let output = Command::new("transmission-show")
        .arg(path.as_os_str())
        .output()
        .context("transmission lanuch failed")?;

    if !output.status.success() {
        bail!("transmission return err");
    }
    let res = std::str::from_utf8(&output.stdout)?;
    let mut lines = res.lines();

    for line in &mut lines {
        if line.trim_start().starts_with("Total Size:") {
            return Ok(line
                .trim_start()
                .strip_prefix("Total Size:")
                .unwrap()
                .to_string());
        } else if line.starts_with("TRACKERS") {
            break;
        }
    }

    bail!("fail to find summary in transmission-show")
}

pub fn base32_hex(content: &str) -> Result<String> {
    let bytes = BASE32_NOPAD.decode(content.as_bytes())?;
    Ok(HEXUPPER.encode(&bytes))
}

pub async fn magnet_info(hash_hex: &str) -> Result<String> {
    let dest = format!("{}/{}.torrent", ROOT_FOLDER, hash_hex.to_ascii_uppercase());
    let dest = Path::new(&dest);

    defer! {
        if dest.exists() {
            let _ = std::fs::remove_file(&dest);
        }
    }

    let hash = hash_hex.to_ascii_uppercase();
    let url = format!("https://itorrents.org/torrent/{}.torrent", hash);
    let response = reqwest::get(&url).await?;

    log::info!("url: {} \n-> {}", url, response.status());

    if response.status().is_success() {
        let mut file = TokioFile::create(dest).await?;
        file.write_all(&response.bytes().await?).await?;
    }
    let res = format!(
        "<code>{}</code>\n---\n总计: {}",
        get_torrent_name_async(dest).await?,
        get_torrent_summary_async(dest).await?,
    );
    Ok(res)
}

pub async fn all_magnet_from_file(input: &Path, output: &Path) -> Result<()> {
    check_input_output(input, output).await?;

    lazy_static! {
        static ref MAGNET_RE: Regex =
            Regex::new(r"magnet:\?xt=urn:btih:([a-fA-F0-9]{40}|[a-zA-Z2-7]{32})").unwrap();
    }
    let mut file = open_without_bom(input).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    let mut iter = MAGNET_RE.captures_iter(&content);
    let mut list = Vec::new();
    for capture in iter.by_ref() {
        list.push(capture[1].to_owned());
    }

    if list.is_empty() {
        bail!("no magnet found");
    } else {
        let mut res = String::new();
        list.iter()
            .for_each(|hash| res.push_str(&format!("magnet:?xt=urn:btih:{}\n", hash)));

        let mut outfile = TokioFile::create(output).await?;
        outfile.write_all(res.as_bytes()).await?;
    }
    Ok(())
}

pub async fn all_ed2k_from_file(input: &Path, output: &Path) -> Result<()> {
    check_input_output(input, output).await?;
    lazy_static! {
        static ref ED2K_RE: Regex =
            Regex::new(r"ed2k://\|file\|[^|]+\|\d+\|[a-fA-F0-9]{32}\|(h=[a-zA-Z2-7]{32}\|)?/")
                .unwrap();
    }

    let mut file = open_without_bom(input).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    // let mut iter =
    let mut res = String::new();
    for mat in ED2K_RE.find_iter(&content) {
        res.push_str(&format!("{}\n", mat.as_str()));
    }

    if res.is_empty() {
        bail!("no ed2k found");
    } else {
        let mut outfile = TokioFile::create(output).await?;
        outfile.write_all(res.as_bytes()).await?;
    }
    Ok(())
}
