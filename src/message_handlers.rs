use crate::{
    global::{Bot, DEBUG_CC_ID, ROOT_FOLDER},
    parsers::{
        all_ed2k_from_file, all_magnet_from_file, all_magnet_from_text, check_dup_n_err,
        file_encoding, file_to_utf8, is_valid_line, json_summary, line_summary,
        path_to_sha1_entity, write_all_to_file, Sha1Entity,
    },
};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use data_encoding::BASE32_NOPAD;
use scopeguard::defer;
use std::{
    fs::remove_file,
    path::{Path, PathBuf},
    time::Duration,
};
use teloxide::{
    net::Download,
    payloads::SendMessageSetters,
    prelude::{Request, Requester, UpdateWithCx},
    requests::HasPayload,
    types::{
        Document, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, Message, MessageEntityKind,
    },
};
use tokio::{fs::File, time::sleep};

fn btn(
    name: impl Into<String>,
    code: impl Into<String>,
    data: impl Into<String>,
) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(name.into(), format!("{}{}", code.into(), data.into()))
}

// save file for debugging
pub async fn copied(bot: &Bot, msg: &Message) -> Result<Message> {
    unsafe {
        if DEBUG_CC_ID == -1 || DEBUG_CC_ID == msg.chat_id() {
            return Err(anyhow!("ignore"));
        }
    }

    let mut req = unsafe { bot.copy_message(DEBUG_CC_ID, msg.chat_id(), msg.id) };

    let mut text_to_send = format!(
        "{}{}",
        msg.text().unwrap_or(""),
        msg.caption().unwrap_or("")
    );

    if let Some(user) = msg.from() {
        text_to_send = format!(
            "{}\n{}:{} @{}",
            text_to_send,
            user.id,
            user.full_name(),
            user.username.as_ref().unwrap_or(&"None".to_string())
        );
    }

    let pl = req.payload_mut();
    pl.caption = Some(format!(
        "{}\n{},@{}",
        text_to_send,
        &msg.chat.id,
        &msg.chat.username().unwrap_or(""),
    ));
    Ok(req.await?)
}

pub async fn download_file(bot: &Bot, doc: &Document) -> Result<PathBuf> {
    let Document {
        file_name, file_id, ..
    } = doc;

    let default_name = "default_name".to_string();
    let file_name = file_name.as_ref().unwrap_or(&default_name);

    let teloxide::types::File { file_path, .. } = bot.get_file(file_id).send().await?;
    let path_str = ROOT_FOLDER.to_owned() + file_id + "." + file_name;
    let path = Path::new(&path_str);
    if path.exists() {
        return Ok(path.to_path_buf());
    }
    let mut new_file = File::create(path).await?;
    bot.download_file(&file_path, &mut new_file).await?;

    Ok(path.to_path_buf())
}

pub async fn line_handler(cx: &UpdateWithCx<Bot, Message>, doc: &Document) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;

    let path = download_file(bot, doc).await?;
    let mut cached = false;

    is_valid_line(&path).await?;

    let summary = line_summary(&path).await.map_err(|e| {
        let _ = std::fs::remove_file(&path);
        e
    })?;

    let _ = copied(bot, msg).await;

    let mut send_str = summary.to_string();
    let mut request = cx.reply_to(&send_str);

    if msg.chat.is_private() {
        let (dup_num, invalid_num) = check_dup_n_err(&path).await?;
        if dup_num == 0 {
            send_str = format!("{}\n恭喜，这个文件没有重复文件链接。", &send_str);
        } else {
            send_str = format!("{}\n! 检测到重复文件 {} 个。", &send_str, dup_num);
        }

        if invalid_num != 0 {
            send_str = format!(
                "{}\n! 包含 {} 个格式不正确的错误链接",
                &send_str, invalid_num
            );
        }

        request = cx.reply_to(&send_str);

        let mut btns = InlineKeyboardMarkup::default();
        let len = doc.file_id.len();
        let last_part: String = doc.file_id.chars().skip(len - 62).collect();

        if summary.has_folder {
            cached = true;
            let btn1 = btn("转成JSON", "2j", &last_part);
            let btn2 = btn("去掉目录信息", "ls", &last_part);
            btns = btns.append_row(vec![btn1, btn2]);
        }

        let btn3 = btn("去除重复/无效文件", "ld", &last_part);
        if dup_num != 0 {
            btns = btns.append_row(vec![btn3]);
            cached = true;
        }
        request = request.reply_markup(btns);
    }

    if !cached {
        let _ = std::fs::remove_file(&path);
    }

    request.await?;
    Ok(())
}

pub async fn json_handler(cx: &UpdateWithCx<Bot, Message>, doc: &Document) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;

    let path = download_file(bot, doc).await?;
    let sha1: Sha1Entity = path_to_sha1_entity(&path).await?;
    let _ = copied(bot, msg).await;
    let summary = json_summary(&sha1).map_err(|e| {
        let _ = std::fs::remove_file(&path);
        e
    })?;

    let mut request = cx.reply_to(summary.to_string());
    if msg.chat.is_private() {
        let len = doc.file_id.len();
        let last_part: String = doc.file_id.chars().skip(len - 62).collect();
        let btns =
            InlineKeyboardMarkup::default().append_row(vec![btn("转成TXT", "2l", last_part)]);
        request = request.reply_markup(btns);
    } else {
        let _ = std::fs::remove_file(&path);
    }
    request.await?;
    Ok(())
}

async fn reply_document_to(
    cx: &UpdateWithCx<Bot, Message>,
    output_path: &Path,
    replied_msg: &Message,
) -> Result<()> {
    let input_file = InputFile::File(output_path.to_path_buf());
    let mut req = cx
        .requester
        .send_document(replied_msg.chat_id(), input_file);

    let payload = req.payload_mut();
    payload.reply_to_message_id = Some(replied_msg.id);
    req.await?;
    Ok(())
}

async fn f_ed2k(cx: &UpdateWithCx<Bot, Message>, replied_msg: &Message) -> Result<()> {
    let doc = if let Some(doc) = replied_msg.document() {
        doc
    } else {
        return Ok(());
    };
    let target_file_path = download_file(&cx.requester, doc).await?;
    let filename = doc
        .file_name
        .to_owned()
        .unwrap_or_else(|| "default_name".to_owned());
    let new_filename = if filename.contains('.') {
        format!(
            "ed2k_{}_{}.txt",
            filename.rsplit_once('.').unwrap().0,
            BASE32_NOPAD.encode(&Utc::now().timestamp_millis().to_ne_bytes())
        )
    } else {
        format!(
            "ed2k_{}_{}.txt",
            filename,
            BASE32_NOPAD.encode(&Utc::now().timestamp_millis().to_ne_bytes())
        )
    };

    let output_path = format!("{}/{}", ROOT_FOLDER, new_filename);
    let output_path = Path::new(&output_path);
    defer! {
        if target_file_path.exists() {
            let _ = remove_file(&target_file_path);
        }
        if output_path.exists(){
            let _ = remove_file(output_path);
        }
    }

    all_ed2k_from_file(&target_file_path, output_path).await?;
    reply_document_to(cx, output_path, replied_msg).await?;
    Ok(())
}

async fn f_magnet(cx: &UpdateWithCx<Bot, Message>, replied_msg: &Message) -> Result<()> {
    let doc = if let Some(doc) = replied_msg.document() {
        doc
    } else {
        return Ok(());
    };
    let target_file_path = download_file(&cx.requester, doc).await?;
    let filename = doc
        .file_name
        .to_owned()
        .unwrap_or_else(|| "default_name".to_owned());
    let new_filename = if filename.contains('.') {
        format!(
            "magnet_{}_{}.txt",
            filename.rsplit_once('.').unwrap().0,
            BASE32_NOPAD.encode(&Utc::now().timestamp_millis().to_ne_bytes())
        )
    } else {
        format!(
            "magnet_{}_{}.txt",
            filename,
            BASE32_NOPAD.encode(&Utc::now().timestamp_millis().to_ne_bytes())
        )
    };

    let output_path = format!("{}/{}", ROOT_FOLDER, new_filename);
    let output_path = Path::new(&output_path);
    defer! {
        if target_file_path.exists() {
            let _ = remove_file(&target_file_path);
        }
        if output_path.exists(){
            let _ = remove_file(output_path);
        }
    }

    all_magnet_from_file(&target_file_path, output_path).await?;
    reply_document_to(cx, output_path, replied_msg).await?;
    Ok(())
}

fn get_urls(msg: &Message) -> Option<Vec<String>> {
    let mut list: Vec<String> = Default::default();

    let entities = if let Some(entities) = msg.entities() {
        entities
    } else {
        msg.caption_entities()?
    };

    for entity in entities {
        if entity.kind == MessageEntityKind::Url {
            if let Some(utf16_repr) = msg.text() {
                let utf16_repr = utf16_repr.encode_utf16().collect::<Vec<u16>>();
                list.push(String::from_utf16_lossy(
                    &utf16_repr[entity.offset..entity.offset + entity.length],
                ));
            }
        }
    }

    if list.is_empty() {
        None
    } else {
        Some(list)
    }
}

async fn w_magnet(cx: &UpdateWithCx<Bot, Message>, replied_msg: &Message) -> Result<()> {
    let urls = get_urls(replied_msg);
    let urls = if let Some(urls) = urls {
        urls
    } else {
        // ignore if there is no urls
        return Ok(());
    };

    let mut list = vec![];

    for url in urls {
        let response = reqwest::get(url).await?.text().await?;
        let all_mag = all_magnet_from_text(&response).await;
        if let Some(all_mag) = all_mag {
            list.extend(all_mag);
        }
    }

    let new_filename = format!(
        "magnet_{}.txt",
        BASE32_NOPAD.encode(&Utc::now().timestamp_millis().to_ne_bytes())
    );
    let output_path = format!("{}/{}", ROOT_FOLDER, new_filename);
    let output_path = Path::new(&output_path);
    defer! {
        if output_path.exists(){
            let _ = remove_file(output_path);
        }
    }

    if list.is_empty() {
        bail!("no magnet found!");
    } else {
        let mut res = String::new();
        list.iter()
            .for_each(|hash| res.push_str(&format!("magnet:?xt=urn:btih:{}\n", hash)));
        write_all_to_file(output_path, res.as_bytes()).await?;
    }

    reply_document_to(cx, output_path, replied_msg).await?;
    Ok(())
}

async fn f_encoding(cx: &UpdateWithCx<Bot, Message>, replied_msg: &Message) -> Result<()> {
    let doc = if let Some(doc) = replied_msg.document() {
        doc
    } else {
        return Ok(());
    };
    let target_file_path = download_file(&cx.requester, doc).await?;

    defer! {
        if target_file_path.exists() {
            let _ = remove_file(&target_file_path);
        }
    }

    let res = file_encoding(&target_file_path).await?;
    let rep = if res.trim() == "unknown" {
        "看不出来啥编码...".to_owned()
    } else {
        format!("编码可能是：{}", res)
    };

    let msg_to_del = cx.reply_to(rep).await?;
    sleep(Duration::from_secs(30)).await;
    cx.requester
        .delete_message(msg_to_del.chat_id(), msg_to_del.id)
        .await?;

    Ok(())
}

async fn f_utf8(cx: &UpdateWithCx<Bot, Message>, replied_msg: &Message) -> Result<()> {
    let doc = if let Some(doc) = replied_msg.document() {
        doc
    } else {
        return Ok(());
    };
    let target_file_path = download_file(&cx.requester, doc).await?;

    let filename = doc
        .file_name
        .to_owned()
        .unwrap_or_else(|| "default_name".to_owned());
    let new_filename = if filename.contains('.') {
        format!(
            "utf8_{}_{}.txt",
            filename.rsplit_once('.').unwrap().0,
            BASE32_NOPAD.encode(&Utc::now().timestamp_millis().to_ne_bytes())
        )
    } else {
        format!(
            "utf8_{}_{}.txt",
            filename,
            BASE32_NOPAD.encode(&Utc::now().timestamp_millis().to_ne_bytes())
        )
    };

    let output_path = format!("{}/{}", ROOT_FOLDER, new_filename);
    let output_path = Path::new(&output_path);
    defer! {
        if target_file_path.exists() {
            let _ = remove_file(&target_file_path);
        }
        if output_path.exists(){
            let _ = remove_file(output_path);
        }
    }
    let res = file_to_utf8(&target_file_path, output_path).await?;

    if res.is_empty() {
        reply_document_to(cx, output_path, replied_msg).await?;
    } else {
        let msg_to_del = cx.reply_to(res).await?;
        sleep(Duration::from_secs(30)).await;
        cx.requester
            .delete_message(msg_to_del.chat_id(), msg_to_del.id)
            .await?;
    }
    Ok(())
}

pub async fn command_check(cx: &UpdateWithCx<Bot, Message>, text: &str) -> Result<()> {
    if !text.starts_with('\'') || cx.update.chat.is_private() {
        // ignore
        return Ok(());
    }

    let replied_msg = if let Some(msg) = cx.update.reply_to_message() {
        msg
    } else {
        return Ok(());
    };

    let now = Utc::now().timestamp();
    let hours_elapsed = (now - replied_msg.date as i64) / 60 / 60;
    // ignore all replies to 24 hours before
    if hours_elapsed > 23 {
        return Ok(());
    }

    match text {
        "'file magnet" | "'f magnet" => f_magnet(cx, replied_msg).await?,
        "'file ed2k" | "'f ed2k" => f_ed2k(cx, replied_msg).await?,
        "'file encoding" | "'f encoding" | "'f 编码" => f_encoding(cx, replied_msg).await?,
        "'file utf8" | "'f utf8" => f_utf8(cx, replied_msg).await?,
        "'webpage magnet" | "'w magnet" => w_magnet(cx, replied_msg).await?,
        _ => {}
    }

    Ok(())
}
