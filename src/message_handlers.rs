use crate::{
    decryption::{format_path_str, preid_decrypt},
    global::{Bot, DEBUG_CC_ID, ROOT_FOLDER},
    parsers::{
        all_ed2k_from_file, all_magnet_from_file, all_magnet_from_text, check_dup_n_err,
        decrypt_line_file, file_encoding, file_to_utf8, is_valid_line, json_summary, line_summary,
        path_to_sha1_entity, write_all_to_file, Sha1Entity, Summary,
    },
};

use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use data_encoding::BASE32_NOPAD;
use lazy_static::lazy_static;
use regex::Regex;
use scopeguard::defer;
use sqlx::{Row, SqlitePool};
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
        Document, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, Message,
        MessageEntityKind,
    },
};
use tokio::{fs::File, time::sleep};
use crate::commands::Command;
use crate::global::{HELP, VERSION};
use crate::parsers::{base32_hex, get_torrent_magnet_async, get_torrent_summary_async, magnet_info, to_iec};
use teloxide::utils::command::BotCommand;

fn btn(
    name: impl Into<String>,
    code: impl Into<String>,
    data: impl Into<String>,
) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(name.into(), format!("{}{}", code.into(), data.into()))
}

// save file for debugging
pub(crate) async fn copied(bot: &Bot, msg: &Message) -> Result<Message> {
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

pub(crate) async fn download_file(bot: &Bot, doc: &Document) -> Result<PathBuf> {
    let mut count = 0;
    loop {
        count += 1;
        if count > 3 {
            bail!("fail to download_file")
        }
        if let Ok(path) = download_file_proxy(bot, doc).await {
            return Ok(path);
        } else {
            //... retry after
            tokio::time::sleep(tokio::time::Duration::from_secs(5 * count)).await;
        }
    }
}

async fn download_file_proxy(bot: &Bot, doc: &Document) -> Result<PathBuf> {
    let Document {
        file_name, file_id, ..
    } = doc;

    let default_name = "default_name".to_string();
    let file_name = file_name.as_ref().unwrap_or(&default_name);

    let teloxide::types::File { file_path, .. } = bot.get_file(file_id).send().await?;
    let path_str = ROOT_FOLDER.to_owned() + file_id + "." + file_name;
    let path = Path::new(&path_str);
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    let mut new_file = File::create(path).await?;
    bot.download_file(&file_path, &mut new_file).await?;

    Ok(path.to_path_buf())
}

pub(crate) async fn line_handler(cx: &UpdateWithCx<Bot, Message>, doc: &Document) -> Result<()> {
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

    if summary.encrypted {
        let old_filename = doc
            .file_name
            .to_owned()
            .unwrap_or_else(|| "default_filename.txt".to_string());

        let new_filename = if old_filename.ends_with(".txt") {
            format!(
                "{}_已解密_{}.txt",
                old_filename.strip_suffix(".txt").unwrap(),
                BASE32_NOPAD.encode(&Utc::now().timestamp().to_ne_bytes())
            )
        } else {
            format!(
                "{}_已解密_{}.txt",
                old_filename,
                BASE32_NOPAD.encode(&Utc::now().timestamp().to_ne_bytes())
            )
        };

        let output_path = format!("{}/{}", ROOT_FOLDER, new_filename);
        let output_path = Path::new(&output_path);
        defer! {
            if path.exists(){
                let _ = remove_file(&path);
            }
            if output_path.exists(){
                let _ = remove_file(output_path);
            }
        }

        decrypt_line_file(&path, output_path).await?;
        send_str = format!("解密完成, {}", send_str);
        reply_document_to(cx, output_path, msg, Some(send_str)).await?;

        return Ok(());
    }

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

pub(crate) async fn json_handler(cx: &UpdateWithCx<Bot, Message>, doc: &Document) -> Result<()> {
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

lazy_static! {
    static ref PATH_ID_REGEX: Regex = Regex::new(r":\d*?/").unwrap();
}

pub(crate) async fn db_handler(cx: &UpdateWithCx<Bot, Message>, doc: &Document) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;
    let db_path = download_file(bot, doc).await?;

    let pool = SqlitePool::connect(&format!("sqlite:{}", db_path.to_string_lossy())).await?;
    let rows = sqlx::query(
    r#"
    SELECT "115://"|| FILENAME || '|' || FILESIZE|| '|' || SHA1, PREID, PATHSTR FROM "myfiles" WHERE SHA1!=0 AND PREID!=0 AND PREID!='error'AND SHA1!='error';
"#).fetch_all(&pool).await?;

    let mut content: String = String::new();

    for row in rows {
        let head = row
            .try_get::<&str, usize>(0)?
            .replace(" ", "_")
            .replace("\\", "")
            .replace("\n", "");
        let preid = preid_decrypt(row.try_get::<&str, usize>(1)?)?;
        let path_str = row.try_get::<&str, usize>(2)?;
        let path_str = format_path_str(path_str)?;
        // let path_str = path_str
        //     .replace(" ", "_")
        //     .replace("\\", "")
        //     .replace("\n", "");

        // let path_str = PATH_ID_REGEX.replace_all(&path_str, "|");
        // let path_str = path_str
        //     .rsplit_once(':')
        //     .ok_or_else(|| anyhow!("wrong db format..."))?
        //     .0;

        content.push_str(&format!("{}|{}|{}\n", head, preid, path_str));
    }

    let filename = doc
        .file_name
        .to_owned()
        .unwrap_or_else(|| "default_name".to_owned());

    let new_filename = if filename.contains('.') {
        format!(
            "{}_sha1导出_{}.txt",
            filename.rsplit_once('.').unwrap().0,
            BASE32_NOPAD.encode(&Utc::now().timestamp().to_ne_bytes())
        )
    } else {
        format!(
            "{}_sha1导出_{}.txt",
            filename,
            BASE32_NOPAD.encode(&Utc::now().timestamp().to_ne_bytes())
        )
    };

    let output_path = format!("{}/{}", ROOT_FOLDER, new_filename);
    let output_path = Path::new(&output_path);
    defer! {
        if db_path.exists() {
            let _ = remove_file(&db_path);
        }
        if output_path.exists(){
            let _ = remove_file(output_path);
        }
    }

    let summary = sqlx::query(
    r#"
    SELECT SUM(FILESIZE), MAX(FILESIZE), MIN(FILESIZE), COUNT(FILESIZE) FROM "myfiles" WHERE SHA1!=0 AND PREID!=0 AND PREID!='error'AND SHA1!='error' ORDER BY "SNO";
"#).fetch_one(&pool).await?;

    let total_size = summary.try_get::<i64, usize>(0)?.try_into()?;
    let max = summary.try_get::<i64, usize>(1)?.try_into()?;
    let min = summary.try_get::<i64, usize>(2)?.try_into()?;
    let total_files = summary.try_get::<i64, usize>(3)?.try_into()?;
    let mid = sqlx::query(
        r#"
SELECT
        AVG(FILESIZE)
FROM (
        SELECT
                FILESIZE
        FROM
                "myfiles"
        WHERE
                SHA1 != 0
                AND PREID != 0
                AND PREID != 'error'
                AND SHA1 != 'error'
        ORDER BY
                FILESIZE
        LIMIT 2 - (
                SELECT
                        COUNT(*)
                FROM
                        "myfiles"
                WHERE
                        SHA1 != 0
                        AND PREID != 0
                        AND PREID != 'error'
                        AND SHA1 != 'error') % 2 -- odd 1, even 2
                OFFSET (
                        SELECT
                                (COUNT(*) - 1) / 2
                        FROM
                                "myfiles"
                        WHERE
                                SHA1 != 0
                                AND PREID != 0
                                AND PREID != 'error'
                                AND SHA1 != 'error'))
              "#,
    )
    .fetch_one(&pool)
    .await?
    .try_get::<f64, usize>(0)?;

    let summary = Summary {
        total_size,
        max,
        min,
        mid,
        total_files,
        has_folder: true,
        encrypted: false,
    };

    if !content.is_empty() {
        let _ = copied(bot, msg).await;
        write_all_to_file(output_path, content.as_bytes()).await?;
        let input_file = InputFile::File(output_path.to_path_buf());
        let mut req = cx.requester.send_document(msg.chat_id(), input_file);
        let payload = req.payload_mut();
        payload.reply_to_message_id = Some(msg.id);
        payload.caption = Some(summary.to_string());
        req.await?;
    }

    pool.close().await;
    Ok(())
}

async fn reply_document_to(
    cx: &UpdateWithCx<Bot, Message>,
    output_path: &Path,
    reply_to: &Message,
    caption: Option<String>,
) -> Result<()> {
    let input_file = InputFile::File(output_path.to_path_buf());
    let mut req = cx.requester.send_document(reply_to.chat_id(), input_file);

    let payload = req.payload_mut();
    payload.reply_to_message_id = Some(reply_to.id);
    payload.caption = caption;
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
            BASE32_NOPAD.encode(&Utc::now().timestamp().to_ne_bytes())
        )
    } else {
        format!(
            "ed2k_{}_{}.txt",
            filename,
            BASE32_NOPAD.encode(&Utc::now().timestamp().to_ne_bytes())
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
    reply_document_to(cx, output_path, replied_msg, None).await?;
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
    reply_document_to(cx, output_path, replied_msg, None).await?;
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

    reply_document_to(cx, output_path, replied_msg, None).await?;
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
        reply_document_to(cx, output_path, replied_msg, None).await?;
    } else {
        let msg_to_del = cx.reply_to(res).await?;
        sleep(Duration::from_secs(30)).await;
        cx.requester
            .delete_message(msg_to_del.chat_id(), msg_to_del.id)
            .await?;
    }
    Ok(())
}

pub(crate) async fn command_check(cx: &UpdateWithCx<Bot, Message>, text: &str) -> Result<()> {
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

async fn link_check(cx: &UpdateWithCx<Bot, Message>, text: &str) -> Result<()> {
    lazy_static! {
        static ref SHA1RE: Regex =
            Regex::new(r"115://(.*?)\|(\d*?)(?:\|[a-fA-F0-9]{40}){2}").unwrap();
    }

    let mut response: String = Default::default();
    let mut counter = 0;
    let mut sum: u128 = 0;
    for cap in SHA1RE.captures_iter(text) {
        counter += 1;
        let size: u128 = cap[2].parse()?;
        sum += size;
        response.push_str(&format!("{} => {}\n", to_iec(size), &cap[1]));
    }

    match counter {
        2.. => response.push_str(&format!("共 {} 个文件, 总计: {}", counter, to_iec(sum))),
        1 => response = format!("文件大小: {}", to_iec(sum)),
        _ => {}
    }

    if !response.is_empty() {
        cx.reply_to(response).await?;
    }

    Ok(())
}

async fn magnet_check(cx: &UpdateWithCx<Bot, Message>, text: &str) -> Result<()> {
    lazy_static! {
        static ref MAGNET_RE: Regex =
            Regex::new(r"magnet:\?xt=urn:btih:([a-fA-F0-9]{40}|[a-zA-Z2-7]{32})").unwrap();
    }
    let mut reply: String = Default::default();
    let hash: String;
    let mut iter = MAGNET_RE.captures_iter(text);
    if let Some(m) = iter.next() {
        if m[1].len() == 40 {
            hash = m[1].to_string();
        } else if m[1].len() == 32 {
            hash = base32_hex(&m[1])?;
        } else {
            unreachable!();
        }
    } else {
        return Ok(());
    }

    if iter.next().is_some() {
        // ignore more than one magnet
        return Ok(());
    }

    reply.push_str(&magnet_info(&hash).await?);
    let mut request = cx.reply_to(reply);
    let payload = request.payload_mut();
    payload.parse_mode = Some(teloxide::types::ParseMode::Html);
    request.await?;

    Ok(())
}


async fn version(cx: &UpdateWithCx<Bot, Message>) -> Result<()> {
    cx.requester
        .send_message(cx.update.chat_id(), VERSION)
        .await?;
    Ok(())
}

async fn help(cx: &UpdateWithCx<Bot, Message>) -> Result<()> {
    cx.requester.send_message(cx.update.chat_id(), HELP).await?;
    Ok(())
}

pub(crate) async fn message_handler(cx: UpdateWithCx<Bot, Message>) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;
    // log::info!("getting a msg!!");

    // if let teloxide::types::MessageKind::NewChatMembers(member) = &msg.kind {
    //     let new_members = &member.new_chat_members;
    //     spam_check(&cx, new_members).await?;
    // }

    // handle command
    let text = if let Some(text) = msg.text() {
        if msg.chat.is_private() {
            match BotCommand::parse(text, "") {
                Ok(Command::Help) => help(&cx).await?,
                Ok(Command::Version) => version(&cx).await?,
                Err(_) => {}
            }
        }
        Some(text)
    } else {
        msg.caption()
    };

    if let Some(text) = text {
        link_check(&cx, text).await?;
        magnet_check(&cx, text).await?;
        command_check(&cx, text).await?;
        // spam_check_dummy(&cx, text).await?;
    }

    if let Some(doc) = msg.document() {
        if let Some(size) = &doc.file_size {
            if *size > 1024 * 1024 * 20 {
                //ignore
                return Ok(());
            }
        }

        if let Some(doc_type) = &doc.mime_type {
            if doc
                .file_name
                .as_ref()
                .unwrap_or(&"".to_string())
                .to_lowercase()
                .ends_with(".db")
            {
                log::info!("getting a db");
                db_handler(&cx, doc).await?;
            } else if *doc_type == mime::TEXT_PLAIN
                || doc
                .file_name
                .as_ref()
                .unwrap_or(&"".to_string())
                .to_lowercase()
                .ends_with(".txt")
            {
                log::info!("getting a txt");
                line_handler(&cx, doc).await?;
            } else if *doc_type == mime::APPLICATION_JSON
                || doc
                .file_name
                .as_ref()
                .unwrap_or(&"".to_string())
                .to_lowercase()
                .ends_with(".json")
            {
                log::info!("getting a json");
                json_handler(&cx, doc).await?;
            } else if *doc_type == "application/x-bittorrent" {
                log::info!("getting a torrent");
                let path = download_file(bot, doc).await?;
                defer! { let _ = std::fs::remove_file(&path); }

                let reply = format!(
                    "<code>{}</code>\n---\n总计: {}",
                    get_torrent_magnet_async(&path).await?,
                    get_torrent_summary_async(&path).await?
                );
                let mut request = cx.reply_to(reply);
                let payload = request.payload_mut();
                payload.parse_mode = Some(teloxide::types::ParseMode::Html);
                request.await?;
            }
        }
    }
    Ok(())
}
