use anyhow::{anyhow, Result};
use anyhow::{bail, Context};
use crypto::{digest::Digest, sha1::Sha1};
use pakr_iec::iec;
use serde::{Deserialize, Serialize};
use serde_bencode::de;
use serde_bytes::ByteBuf;
use std::fmt;

use std::io::SeekFrom;
use std::path::Path;
use std::str::FromStr;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter};

pub fn json2line(input: &Path, output: &Path) -> Result<()> {
    if !input.exists() {
        bail!("input not found");
    }

    if output.exists() {
        bail!("output path taken");
    }

    let entity = input.to_str().unwrap().parse::<Sha1Entity>()?;

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

pub async fn line2json(input: &Path, output: &Path) -> Result<()> {
    if !input.exists() {
        bail!("input not found");
    }

    if output.exists() {
        bail!("output path taken");
    }

    let mut root_list: Vec<Sha1Entity> = Vec::new();
    let mut file = TokioFile::open(input)
        .await
        .context(format!("failed to open {}", input.to_string_lossy()))?;

    if has_bom_async(input).await? {
        file.seek(SeekFrom::Start(3)).await?;
    } else {
    }

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await.context(format!(
        "failed to read line from file: {}",
        input.to_string_lossy()
    ))? {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() <= 4 {
            return Err(WrongSha1LinkFormat.into());
        }
        let name = parts[0].to_owned();
        let size = parts[1].parse()?;
        let sha1 = parts[2].to_owned();
        let sha1_block = parts[3].to_owned();

        let file = FileRepr {
            name,
            size,
            sha1,
            sha1_block,
            id: None,
        };
        let mut curr_list: &mut Vec<_> = &mut root_list;
        for part in parts.iter().take(parts.len() - 1).skip(4) {
            let folder = get_dir_or_create(part, curr_list);
            curr_list = &mut folder.dirs;
        }
        let folder = get_dir_or_create(parts[parts.len() - 1], curr_list);
        folder.files.push(file);
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

    Ok(())
}

pub fn line_summary_mem(content: &str) -> Result<Summary> {
    let mut all_size: Vec<u64> = Vec::new();
    let mut num_lines: u64 = 0;
    let mut has_folder = true;

    for line in content.lines() {
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
        bail!("failed to read file");
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
    let mut all_size: Vec<u64> = Vec::new();
    let mut num_lines: u64 = 0;
    let mut has_folder = true;

    let file = TokioFile::open(path).await?;
    // let meta = file.metadata().await?;
    // dbg!(meta.len());

    let reader = tokio::io::BufReader::new(file);
    // dbg!(&reader);
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await.expect("Failed to read file") {
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

fn to_iec(num: impl Into<u128>) -> String {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Sha1Entity {
    dir_name: String,
    files: Vec<FileRepr>,
    dirs: Vec<Self>,
    #[serde(skip_serializing)]
    id: Option<u64>,
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

#[derive(Debug)]
pub struct FileRepr {
    name: String,
    size: u64,
    sha1: String,
    sha1_block: String,
    id: Option<u64>,
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
struct WrongSha1LinkFormat;

impl std::fmt::Display for WrongSha1LinkFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wrong format of 115sha1link")
    }
}

impl std::error::Error for WrongSha1LinkFormat {}

use serde::de::Error;
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

fn has_bom(path: &Path) -> Result<bool> {
    let file =
        std::fs::File::open(path).context(format!("failed to open {}", path.to_string_lossy()))?;
    let reader = std::io::BufReader::new(file);
    let mut buffer = [0; 3];
    let mut content = std::io::Read::take(reader, 3);
    let num_reads =
        std::io::Read::read(&mut content, &mut buffer).context("fail to read bytes from file")?;
    Ok(num_reads < 3 || buffer == [0xef, 0xbb, 0xbf])
}

async fn has_bom_async(path: &Path) -> Result<bool> {
    let file = TokioFile::open(path)
        .await
        .context(format!("failed to open {}", path.to_string_lossy()))?;
    let reader = BufReader::new(file);
    let mut buffer = [0; 3];
    let mut content = reader.take(3);
    let num_reads = content
        .read(&mut buffer)
        .await
        .context("fail to read bytes from file")?;
    Ok(num_reads < 3 || buffer == [0xef, 0xbb, 0xbf])
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
// async fn write_line_async(
//     writer: &mut BufWriter<TokioFile>,
//     entity: &Sha1Entity,
//     suffix: String,
// ) -> Result<()> {
//     let suffix = suffix + "|" + &entity.dir_name;
//     for file in &entity.files {
//         let link = file.to_sha1_link();
//         let line = link + &suffix + "\n";
//         let mut buffer = line.as_bytes();
//         writer.write_all_buf(&mut buffer).await?;
//     }
//     for dir in &entity.dirs {
//         write_line_async(writer, dir, suffix.to_owned()).await?;
//     }

//     Ok(())
// }

#[derive(Debug, Deserialize)]
struct Node(String, i64);

#[derive(Debug, Deserialize, Serialize)]
struct File {
    path: Vec<String>,
    length: i64,
    #[serde(default)]
    md5sum: Option<String>,
}

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

#[derive(Debug, Deserialize)]
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

pub fn get_torrent_magnet(path: &Path) -> Result<String> {
    let path = Path::new(path);
    if !path.exists() {
        bail!("file not found...");
    }

    let mut file = std::fs::File::open(path).context("fail to open file")?;
    let metadata = file
        .metadata()
        .context(format!("fail to get metadata for {:?}", file))?;
    let mut buf = vec![0; metadata.len() as usize];
    std::io::Read::read(&mut file, &mut buf).context("fail to read file as bytes")?;

    let torrent = de::from_bytes::<Torrent>(&buf).context("bencode deserialization failed.")?;
    let bytes = serde_bencode::to_bytes(&torrent.info)?;
    let mut hasher = Sha1::new();
    hasher.input(&bytes);
    let res = hasher.result_str();

    Ok("magnet:?xt=urn:btih:".to_string() + &res)
}

pub async fn get_torrent_magnet_async(path: &Path) -> Result<String> {
    let path = Path::new(path);
    if !path.exists() {
        bail!("file not found...");
    }

    let mut file = TokioFile::open(path).await.context("fail to open file")?;
    let mut buf = Vec::new();

    file.read_to_end(&mut buf).await?;

    let torrent = de::from_bytes::<Torrent>(&buf).context("bencode deserialization failed.")?;
    let bytes = serde_bencode::to_bytes(&torrent.info)?;
    let mut hasher = Sha1::new();
    hasher.input(&bytes);
    let res = hasher.result_str();

    Ok("magnet:?xt=urn:btih:".to_string() + &res)
}
