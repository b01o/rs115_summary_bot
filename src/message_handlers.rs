use crate::{
    global::{DEBUG_CC_ID, ROOT_FOLDER},
    parsers::{
        check_dup_n_err, is_valid_line, json_summary, line_summary, path_to_sha1_entity, Sha1Entity,
    },
};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use teloxide::{
    adaptors::AutoSend,
    net::Download,
    payloads::SendMessageSetters,
    prelude::{Request, Requester, UpdateWithCx},
    requests::HasPayload,
    types::{Document, InlineKeyboardButton, InlineKeyboardMarkup, Message},
    Bot,
};
use tokio::fs::File;

fn btn(
    name: impl Into<String>,
    code: impl Into<String>,
    data: impl Into<String>,
) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(name.into(), format!("{}{}", code.into(), data.into()))
}

// save file for debugging
pub async fn copied(bot: &AutoSend<Bot>, msg: &Message) -> Result<Message> {
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

pub async fn download_file(bot: &AutoSend<Bot>, doc: &Document) -> Result<PathBuf> {
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

pub async fn line_handler(cx: &UpdateWithCx<AutoSend<Bot>, Message>, doc: &Document) -> Result<()> {
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

pub async fn json_handler(cx: &UpdateWithCx<AutoSend<Bot>, Message>, doc: &Document) -> Result<()> {
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
