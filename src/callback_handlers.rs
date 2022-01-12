use crate::{
    global::*,
    parsers::{dedup_filerepr_file, json2line, line2json, line_strip_dir_info},
};
use anyhow::Result;
use scopeguard::defer;
use std::path::{Path, PathBuf};
use teloxide::{
    prelude::Requester,
    requests::HasPayload,
    types::{InputFile, Message},
};


use teloxide::prelude::{CallbackQuery, UpdateWithCx};
use tokio::fs::read_dir;

struct CacheFile {
    name: String,
    path: PathBuf,
}

async fn find_cache(id_suffix: &str) -> Result<Option<CacheFile>> {
    let root = Path::new(ROOT_FOLDER);
    let mut paths = read_dir(root).await?;

    while let Some(dir) = paths.next_entry().await? {
        let filename = dir.file_name();
        let filename = filename.to_string_lossy();
        let arr: Vec<&str> = filename.splitn(2, '.').collect();
        if arr.len() != 2 {
            continue;
        }
        let file_id = arr[0];
        if file_id.ends_with(id_suffix) {
            let path = dir.path();
            let mut name = dir.file_name().to_string_lossy().to_string();
            name = name
                .splitn(2, '.')
                .nth(1)
                .unwrap_or("default_name")
                .to_string();
            return Ok(Some(CacheFile { name, path }));
        }
    }
    Ok(None)
}

pub(crate) async fn callback_to_dedup(bot: &Bot, msg: &Message, id_suffix: &str) -> Result<bool> {
    let mut found_cache = false;
    if let Some(cache) = find_cache(id_suffix).await? {
        found_cache = true;
        let filename = &cache.name;
        let mut new_file_path = cache.path.clone();
        new_file_path.pop();
        let new_filename: String;

        if filename.ends_with(".txt") {
            new_filename = filename[..filename.len() - 4].to_string() + "_已去重" + ".txt";
        } else {
            new_filename = filename.to_string() + "_已去重" + ".txt";
        }

        new_file_path.push(new_filename);
        defer! {
            if cache.path.exists(){
                let _ = std::fs::remove_file(&cache.path);
            }
            if new_file_path.exists(){
                let _ = std::fs::remove_file(&new_file_path);
            }
        }

        dedup_filerepr_file(&cache.path, &new_file_path).await?;

        let input_file = InputFile::File(new_file_path.to_path_buf());
        let mut req = bot.send_document(msg.chat_id(), input_file);
        let payload = req.payload_mut();
        payload.reply_to_message_id = Some(msg.id);
        req.await?;
    }
    Ok(found_cache)
}

pub(crate) async fn callback_to_line(bot: &Bot, msg: &Message, id_suffix: &str) -> Result<bool> {
    let mut found_cache = false;
    if let Some(cache) = find_cache(id_suffix).await? {
        found_cache = true;
        let filename = &cache.name;
        let mut new_file_path = cache.path.clone();
        new_file_path.pop();
        let new_filename: String;
        if filename.ends_with(".json") {
            new_filename = filename[..filename.len() - 5].to_string() + ".txt";
        } else {
            new_filename = filename.to_string() + ".txt";
        }
        new_file_path.push(new_filename);

        defer! {
            if cache.path.exists(){
                let _ = std::fs::remove_file(&cache.path);
            }
            if new_file_path.exists(){
                let _ = std::fs::remove_file(&new_file_path);
            }
        }

        json2line(&cache.path, &new_file_path).await?;

        let input_file = InputFile::File(new_file_path.to_path_buf());
        let mut req = bot.send_document(msg.chat_id(), input_file);
        let payload = req.payload_mut();
        payload.reply_to_message_id = Some(msg.id);
        req.await?;
    }
    Ok(found_cache)
}

pub(crate) async fn callback_to_json(bot: &Bot, msg: &Message, id_suffix: &str) -> Result<bool> {
    let mut found_cache = false;
    if let Some(cache) = find_cache(id_suffix).await? {
        found_cache = true;
        let filename = &cache.name;
        let mut new_file_path = cache.path.clone();
        new_file_path.pop();
        let new_filename: String;
        if filename.ends_with(".txt") {
            new_filename = filename[..filename.len() - 4].to_string() + ".json";
        } else {
            new_filename = filename.to_string() + ".json";
        }
        new_file_path.push(new_filename);

        defer! {
            if cache.path.exists(){
                let _ = std::fs::remove_file(&cache.path);
            }
            if new_file_path.exists(){
                let _ = std::fs::remove_file(&new_file_path);
            }
        }

        line2json(&cache.path, &new_file_path).await?;

        let input_file = InputFile::File(new_file_path.to_path_buf());
        let mut req = bot.send_document(msg.chat_id(), input_file);
        let payload = req.payload_mut();
        payload.reply_to_message_id = Some(msg.id);
        req.await?;
    }
    Ok(found_cache)
}

pub(crate) async fn callback_line_strip_dir(bot: &Bot, msg: &Message, id_suffix: &str) -> Result<bool> {
    let mut found_cache = false;
    if let Some(cache) = find_cache(id_suffix).await? {
        found_cache = true;
        let filename = &cache.name;
        let mut new_file_path = cache.path.clone();
        new_file_path.pop();
        let new_filename: String;

        if filename.ends_with(".txt") {
            new_filename = filename.clone();
        } else {
            new_filename = filename.to_string() + ".txt";
        }
        new_file_path.push(new_filename);

        defer! {
            if cache.path.exists(){
                let _ = std::fs::remove_file(&cache.path);
            }
            if new_file_path.exists(){
                let _ = std::fs::remove_file(&new_file_path);
            }
        }

        line_strip_dir_info(&cache.path, &new_file_path).await?;

        let input_file = InputFile::File(new_file_path.to_path_buf());
        let mut req = bot.send_document(msg.chat_id(), input_file);
        let payload = req.payload_mut();
        payload.reply_to_message_id = Some(msg.id);
        req.await?;
    }
    Ok(found_cache)
}

pub(crate) async fn callback_handler(cx: UpdateWithCx<Bot, CallbackQuery>) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: query,
    } = &cx;

    if let (Some(version), Some(msg)) = (&query.data, &query.message) {
        let working = "请稍等...";
        let to_send = format!("{}\n{}", msg.text().unwrap_or(""), working);
        bot.edit_message_text(msg.chat.id, msg.id, &to_send).await?;

        let found_cache = match &version[..2] {
            "2j" => callback_to_json(bot, msg, &version[2..]).await?,
            "2l" => callback_to_line(bot, msg, &version[2..]).await?,
            "ls" => callback_line_strip_dir(bot, msg, &version[2..]).await?,
            "ld" => callback_to_dedup(bot, msg, &version[2..]).await?,
            _ => {
                bot.answer_callback_query(&query.id).await?;
                let text = msg.text().unwrap_or("").to_owned() + "\n发生了错误..";
                bot.edit_message_text(msg.chat.id, msg.id, text).await?;
                return Ok(());
            }
        };

        bot.answer_callback_query(&query.id).await?;
        // query.inline_message_id

        if !found_cache {
            let mut req = bot.send_message(msg.chat_id(), "文件已过期，请重新发送");
            let payload = req.payload_mut();
            payload.reply_to_message_id = Some(msg.id);
            req.await?;
        }

        let text = msg.text().unwrap_or("");
        bot.edit_message_text(msg.chat.id, msg.id, text).await?;
    }

    Ok(())
}