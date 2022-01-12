use anyhow::{bail, Context, Result};
use std::{io::SeekFrom, path::Path};
use tokio::{
    fs::File as TokioFile,
    io::{AsyncReadExt, AsyncSeekExt, BufReader},
};

pub(crate) fn has_bom(path: &Path) -> Result<bool> {
    let file =
        std::fs::File::open(path).context(format!("failed to open {}", path.to_string_lossy()))?;
    let reader = std::io::BufReader::new(file);
    let mut buffer = [0; 3];
    let mut content = std::io::Read::take(reader, 3);
    let num_reads =
        std::io::Read::read(&mut content, &mut buffer).context("fail to read bytes from file")?;
    Ok(num_reads < 3 || buffer == [0xef, 0xbb, 0xbf])
}

pub(crate) async fn has_bom_async(path: &Path) -> Result<bool> {
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

pub(crate) async fn open_without_bom(input: &Path) -> Result<TokioFile> {
    let mut file = TokioFile::open(input)
        .await
        .context(format!("failed to open {}", input.to_string_lossy()))?;
    if has_bom_async(input).await? {
        file.seek(SeekFrom::Start(3)).await?;
    }
    Ok(file)
}

pub(crate) async fn check_input(input: &Path) -> Result<()> {
    if !input.exists() {
        bail!("input not found");
    }
    Ok(())
}

pub(crate) async fn check_input_output(input: &Path, output: &Path) -> Result<()> {
    check_input(input).await?;
    if output.exists() {
        bail!("output path taken");
    }
    Ok(())
}
