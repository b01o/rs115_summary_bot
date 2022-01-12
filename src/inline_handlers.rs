use crate::global::Bot;
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
        let title = if let Some(filename) = record.filename {
            if let Some(text) = record.text {
                description = format!("文件名：{}\n消息内容：{}", filename, text);
            } else {
                description = format!("文件名：{}\n", filename);
            }
            msg = "📦文件 | ";
            "[聊天记录|📦文件]".to_string()
        } else {
            description = format!(
                "文本：{}",
                record.text.unwrap_or("default text".to_string())
            );
            msg = "📃普通消息 | ";
            "[聊天记录|📃普通消息]".to_string()
        };

        let imct_text = format!("{}<a href=\"{}\">{}</a>", msg, &url, query.query);
        let mut imct = InputMessageContentText::new(imct_text);
        imct.parse_mode = Some(ParseMode::Html);
        results.push(InlineQueryResult::Article(InlineQueryResultArticle::new(
            url.clone(),
            title,
            InputMessageContent::Text(imct))
            .description(description)
            .hide_url(true)
            .thumb_url("https://user-images.githubusercontent.com/16791440/148683429-efd6451e-4d7a-420f-966c-cedb8b79b22b.png")
            .url(url)

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
            )
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
