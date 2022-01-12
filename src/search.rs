#![allow(dead_code)]
///
/// this file provides abilities to search through CJK chat history on telegram
///
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, LoadExtensionGuard};

use std::path::Path;
use std::process::{Child, Command};



const LIB_PATH: &str = "./libsimple";
const DB_PATH: &str = "./tg_archive.db";
const ARCHIVER_SCRIPT_PATH: &str = "./scripts/tg_chat_archiver.py";

fn load_my_extension(conn: &Connection) -> Result<()> {
    unsafe {
        let _guard = LoadExtensionGuard::new(conn)?;
        conn.load_extension(LIB_PATH, None)
            .context("load extension failed")
    }
}

#[derive(Debug)]
pub(crate) struct Record {
    pub(crate) id: u64,
    pub(crate) text: Option<String>,
    pub(crate) filename: Option<String>,
}
fn to_record(id: u64, text: Option<String>, filename: Option<String>) -> rusqlite::Result<Record> {
    Ok(Record { id, text, filename })
}

fn create_table_if_not_exist(conn: &Connection) -> Result<()> {
    let create1_sql = r##" CREATE VIRTUAL TABLE IF NOT EXISTS message_text USING fts5(text,tokenize = 'simple');"##;
    let create2_sql = r##"CREATE VIRTUAL TABLE IF NOT EXISTS message_filename USING fts5(text,tokenize = 'simple');"##;
    conn.execute(create1_sql, [])?;
    conn.execute(create2_sql, [])?;
    let create3_sql = r##"CREATE TABLE IF NOT EXISTS archive
(
    id                  INTEGER,
    sender_id           INTEGER,
    chat_id             INTEGER NOT NULL,
    type                INTEGER NOT NULL,
    message_text_id     INTEGER REFERENCES message_text (ROWID),
    message_filename_id INTEGER REFERENCES message_filename (ROWID),
    filesize            INTEGER,
    ext                 TEXT,
    create_time         INTEGER,
    edit_time           INTEGER,
    UNIQUE(id, edit_time) ON CONFLICT IGNORE
);"##;
    conn.execute(create3_sql, [])?;

    Ok(())
}

pub(crate) struct Librarian {
    conn: Connection,
    task: Option<Child>,
}

impl Librarian {
    pub(crate) fn new() -> Result<Librarian> {
        let db_location = Path::new(DB_PATH);
        let conn = Connection::open(db_location)?;
        load_my_extension(&conn)?;
        create_table_if_not_exist(&conn)?;
        Ok(Librarian { conn, task: None })
    }

    pub(crate) async fn is_ready_for_chat(&self, chat: &str) -> Result<bool> {
        let output = Command::new("python3")
            .arg(ARCHIVER_SCRIPT_PATH)
            .arg("--check")
            .arg("--target_chat")
            .arg(chat)
            .output()
            .context("archiver start failed")?;

        if output.status.success() {
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) async fn archive(&mut self, chat: &str) -> Result<()> {
        let ready = self.is_ready_for_chat(chat).await?;
        if !ready {
            bail!("not for this chat")
        }

        let child = Command::new("python3")
            .arg(ARCHIVER_SCRIPT_PATH)
            .arg("--target_chat")
            .arg(chat)
            .arg("--db")
            .arg("./test.db")
            .spawn()
            .context("archiver start failed")?;
        self.task = Some(child);
        Ok(())
    }

    pub(crate) fn is_currently_available(&mut self) -> Result<bool> {
        if let Some(ref mut task) = self.task {
            match task.try_wait() {
                Ok(None) => Ok(false),
                Ok(Some(_status)) => Ok(true),
                Err(_e) => {
                    bail!("err")
                }
            }
        } else {
            Ok(true)
        }
    }
    pub(crate) fn search_files(
        &self,
        keyword: &str,
        chat_id: i64,
        limit: u64,
        page_num: u64,
    ) -> Result<Vec<Record>> {
        let search_sql = r##"
       select id, message_filename.text as filename, message_text.text as text
from archive
         left join message_filename on archive.message_filename_id = message_filename.ROWID
         left join message_text on archive.message_text_id = message_text.ROWID
where chat_id=? and type=5 and id in
      (select id
       from archive
       where message_filename_id in
             (select ROWID
              from message_filename
              where text match simple_query(?)
             )
       union
       select id
       from archive
       where message_text_id in
             (select ROWID
              from message_text
              where text match simple_query(?)
             )) order by id desc limit ? offset ?
        "##;
        let mut stmt = self.conn.prepare(search_sql)?;
        let rows = stmt.query_map(
            params![chat_id, keyword, keyword, limit, page_num * limit],
            |row| to_record(row.get(0)?, row.get(2)?, row.get(1)?),
        )?;
        let list: Vec<_> = rows.flat_map(|x|x.ok())
            .collect();
        Ok(list)

    }


    pub(crate) fn search(
        &self,
        keyword: &str,
        chat_id: i64,
        limit: u64,
        page_num: u64,
    ) -> Result<Vec<Record>> {
        let search_sql = r##"
       select id, message_filename.text as filename, message_text.text as text
from archive
         left join message_filename on archive.message_filename_id = message_filename.ROWID
         left join message_text on archive.message_text_id = message_text.ROWID
where chat_id=? and id in
      (select id
       from archive
       where message_filename_id in
             (select ROWID
              from message_filename
              where text match simple_query(?)
             )
       union
       select id
       from archive
       where message_text_id in
             (select ROWID
              from message_text
              where text match simple_query(?)
             )) order by id desc limit ? offset ?
        "##;
        let mut stmt = self.conn.prepare(search_sql)?;
        let rows = stmt.query_map(
            params![chat_id, keyword, keyword, limit, page_num * limit],
            |row| to_record(row.get(0)?, row.get(2)?, row.get(1)?),
        )?;
        let list: Vec<_> = rows.flat_map(|x|x.ok())
            .collect();
        Ok(list)
    }

    //(id, sender_id, chat_id, type, text, filename, filesize, ext, create_time, edit_time)
    pub(crate) fn index_a_message(
        &self,
        id: i64,
        sender_id: Option<i64>,
        chat_id: i64,
        _type: u8,
        text: Option<&str>,
        filename: Option<&str>,
        filesize: Option<u64>,
        ext: Option<String>,
        create_time: Option<u64>,
        edit_time: Option<u64>,
    ) -> Result<()> {
        let mut text_id: Option<i64> = None;
        if let Some(text) = text {
            let mut stmt = self
                .conn
                .prepare(r##"SELECT ROWID FROM message_text WHERE text=?;"##)?;
            let mut output = stmt.query(params![text])?;
            if let Some(row) = output.next()? {
                text_id = Some(row.get(0)?);
            } else {
                let mut stmt = self
                    .conn
                    .prepare(r##"INSERT INTO message_text VALUES (?) ;"##)?;
                stmt.execute(params![text])?;
                text_id = Some(self.conn.last_insert_rowid());
                self.conn.flush_prepared_statement_cache();
            }
        }
        let mut filename_id: Option<i64> = None;
        if let Some(filename) = filename {
            let mut stmt = self
                .conn
                .prepare(r##"SELECT ROWID FROM message_filename WHERE text=?;"##)?;
            let mut output = stmt.query(params![filename])?;
            if let Some(row) = output.next()? {
                filename_id = Some(row.get(0)?);
            } else {
                let mut stmt = self
                    .conn
                    .prepare(r##"INSERT INTO message_filename VALUES (?) ;"##)?;
                stmt.execute(params![filename])?;
                filename_id = Some(self.conn.last_insert_rowid());
                self.conn.flush_prepared_statement_cache();
            }
        }

        let sql_insert = r#"INSERT INTO archive VALUES (?,?,?,?,?,?,?,?,?,?);"#;
        //(id, sender_id, chat_id, type, text, filename, filesize, ext, create_time, edit_time)
        self.conn.execute(
            sql_insert,
            params![
                id,
                sender_id,
                chat_id,
                _type,
                text_id,
                filename_id,
                filesize,
                ext,
                create_time,
                edit_time
            ],
        )?;
        Ok(())
    }
}
