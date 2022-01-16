use crate::global::Bot;
use crate::global::*;
use crate::search::Librarian;
use anyhow::Result;
use std::sync::Arc;
use teloxide::prelude::{Requester, UpdateWithCx};
use teloxide::requests::HasPayload;
use teloxide::types::{
    InlineQuery, InlineQueryResult, InlineQueryResultArticle, InputMessageContent,
    InputMessageContentText, ParseMode,
};
use tokio::sync::Mutex;

pub(crate) async fn inline_query_handler(
    cx: UpdateWithCx<Bot, InlineQuery>,
    librarian: Arc<Mutex<Librarian>>,
) -> Result<()> {
    let UpdateWithCx {
        requester: _bot,
        update: query,
    } = &cx;
    if query.query.is_empty()
        || query.query.chars().all(char::is_whitespace)
        || query.query.trim() == "file"
    {
        // ignore
        return Ok(());
    }
    let search_query = query.query.trim();
    let offset = if query.offset.is_empty() {
        0u64
    } else {
        query.offset.parse()?
    };
    let list;
    if search_query.starts_with("file ") {
        list = librarian.lock().await.search_files(
            search_query.strip_prefix("file ").unwrap(),
            1405404182,
            50,
            offset,
        )?;
    } else {
        list = librarian
            .lock()
            .await
            .search(search_query, 1405404182, 50, offset)?;
    }

    let mut results = vec![];
    let prefix = "https://t.me/Resources115/";
    for record in list {
        let url = format!("{}{}", prefix, record.id);
        let description;
        let msg;
        //if message.gif:
        //                     type = 1
        //                 elif message.sticker:
        //                     type = 2
        //                 elif message.photo:
        //                     type = 3
        //                 elif message.video:
        //                     type = 4
        //                 elif message.document:
        //                     type = 5
        //                 else:
        //                     type = 0
        let mut thumb_url = match record.kind {
            1 => {
                msg = "👾 GIF | ";
                IMG_KIND_GIF
            }
            2 => {
                msg = "🔖 贴图 | ";
                IMG_KIND_FILE_STICKERS
            }
            3 => {
                msg = "🖼 图片 | ";
                IMG_KIND_FILE_PICTURE
            }

            4 => {
                msg = "🎞 视频 | ";
                IMG_KIND_FILE_VIDEO
            }
            5 => {
                msg = "📦 文件 | ";
                IMG_KIND_FILE_OTHERS
            }
            0 => {
                msg = "✉️ 普通消息 | ";
                IMG_KIND_NORMAL
            }

            _ => {
                panic!("kinds that does not exists!")
            }
        };

        thumb_url = if let Some(filename) = &record.filename {
            if let Some(parts) = filename.rsplit_once(".") {
                match parts.1.to_uppercase().as_str() {
                    "7Z" => IMG_KIND_7Z,
                    "APK" => IMG_KIND_APK,
                    "ASS" => IMG_KIND_ASS,
                    "BAT" => IMG_KIND_BAT,
                    "CRX" => IMG_KIND_CRX,
                    "CSV" => IMG_KIND_CSV,
                    "DB" => IMG_KIND_DB,
                    "DOC" => IMG_KIND_DOC,
                    "DOCX" => IMG_KIND_DOCX,
                    "EOF" => IMG_KIND_EOF,
                    "EPUB" => IMG_KIND_EPUB,
                    "EXE" => IMG_KIND_EXE,
                    "GIF" => IMG_KIND_FILE_GIF,
                    "HTML" => IMG_KIND_HTML,
                    "JPG" => IMG_KIND_JPG,
                    "JS" => IMG_KIND_JS,
                    "JSON" => IMG_KIND_JSON,
                    "M3U" => IMG_KIND_M3U,
                    "MKV" => IMG_KIND_MKV,
                    "MOBI" => IMG_KIND_MOBI,
                    "MP4" => IMG_KIND_MP4,
                    "PDF" => IMG_KIND_PDF,
                    "PNG" => IMG_KIND_PNG,
                    "RAR" => IMG_KIND_RAR,
                    "SRT" => IMG_KIND_SRT,
                    "SSA" => IMG_KIND_SSA,
                    "TORRENT" => IMG_KIND_TORRENT,
                    "TXT" => IMG_KIND_TXT,
                    "WMV" => IMG_KIND_WMV,
                    "XLS" => IMG_KIND_XLS,
                    "XLSX" => IMG_KIND_XLSX,
                    "YAML" => IMG_KIND_YAML,
                    "ZIP" => IMG_KIND_ZIP,
                    "AZW3" => IMG_KIND_AZW3,
                    _ => thumb_url,
                }
            } else {
                // does not have ext
                IMG_KIND_FILE_OTHERS
            }
        } else {
            thumb_url
        };

        let title = format!("{}", msg.strip_suffix(" | ").unwrap());
        if let Some(filename) = record.filename {
            if let Some(text) = record.text {
                description = format!("文件名：{}\n消息内容：{}", filename, text);
            } else {
                description = format!("文件名：{}\n", filename);
            }
        } else {
            description = format!(
                "文本：{}",
                record.text.unwrap_or("default text".to_string())
            );
        }

        let imct_text = format!("{}<a href=\"{}\">{}</a>", msg, &url, query.query);
        let mut imct = InputMessageContentText::new(imct_text);
        imct.parse_mode = Some(ParseMode::Html);
        results.push(InlineQueryResult::Article(
            InlineQueryResultArticle::new(url.clone(), title, InputMessageContent::Text(imct))
                .description(description)
                .hide_url(true)
                .thumb_url(thumb_url.to_owned())
                .url(url),
        ))
    }
    if !results.is_empty() {
        let mut req = cx.requester.answer_inline_query(&query.id, results);
        let payload = req.payload_mut();
        payload.is_personal = Some(false);
        payload.next_offset = Some((offset + 1).to_string());
        payload.cache_time = Some(600);
        req.await?;
    } else {
        let line = if offset == 0 {
            InlineQueryResultArticle::new(
                "-1".to_string(),
                "未找到符合条件的消息".to_string(),
                InputMessageContent::Text(InputMessageContentText::new("/delete_this")),
            )
        } else {
            InlineQueryResultArticle::new(
                "0".to_string(),
                "- 已经到底了 -".to_string(),
                InputMessageContent::Text(InputMessageContentText::new("/delete_this")),
            ).thumb_url(IMG_KIND_EOF)
        };

        results.push(InlineQueryResult::Article(line));
        let mut req = cx.requester.answer_inline_query(&query.id, results);
        let payload = req.payload_mut();
        payload.is_personal = Some(false);
        payload.cache_time = Some(600);
        req.await?;
    }
    Ok(())
}
