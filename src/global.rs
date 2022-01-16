#![allow(dead_code)]
use teloxide::adaptors::{AutoSend, Throttle};
pub(crate) type Bot = AutoSend<Throttle<teloxide::Bot>>;

pub(crate) const ROOT_FOLDER: &str = ".cache/tgtmp/";
pub(crate) static mut DEBUG_CC_ID: i64 = -1;
pub(crate) const HELP: &str = r"使用方法: 向机器人发送 sha1 文件, 出现对应选项。
FAQ:
1. json 文件需要以 .json 文件名后缀结尾，否则忽略。
2. 仅含有目录信息的 txt 才支持转换成 json 文件。
3. 目前去重和除错功能仅对 txt 格式的 115sha1 文件有效, 请需要进行相应操作用 line 格式的 txt 文件
4. 目前仅支持20M内的文件。
5. 有问题群里@我

更多详细内容：https://telegra.ph/het-12-01";

pub(crate) const VERSION: &str = "2.5.0 Jan 16 2022 CST 测试搜索中";

pub(crate) const IMG_KIND_7Z: &str = "https://user-images.githubusercontent.com/16791440/149652329-b381b6b3-8588-4730-9f5f-325afa162fe3.png";
pub(crate) const IMG_KIND_APK: &str = "https://user-images.githubusercontent.com/16791440/149652337-2ce296dd-ff61-4d9d-9c20-97b5277391df.png";
pub(crate) const IMG_KIND_ASS: &str = "https://user-images.githubusercontent.com/16791440/149652340-b0b1d575-1113-48a0-b196-cc17cbf48f5d.png";
pub(crate) const IMG_KIND_AZW3: &str = "https://user-images.githubusercontent.com/16791440/149652341-2821e5ff-0e02-4c00-bb77-81c059e1f379.png";
pub(crate) const IMG_KIND_BAT: &str = "https://user-images.githubusercontent.com/16791440/149652418-c5fab566-a3a9-48d6-bb99-7970ddc4f44a.png";
pub(crate) const IMG_KIND_CRX: &str = "https://user-images.githubusercontent.com/16791440/149652421-48918a28-10cb-4727-b4c4-84facbfbc5c4.png";
pub(crate) const IMG_KIND_CSV: &str = "https://user-images.githubusercontent.com/16791440/149652422-689480f0-56b8-45d2-9c16-aac3e7bf67fc.png";
pub(crate) const IMG_KIND_DB: &str = "https://user-images.githubusercontent.com/16791440/149652424-7777d3ab-969e-406e-a242-cc338abf53d3.png";
pub(crate) const IMG_KIND_DOC: &str = "https://user-images.githubusercontent.com/16791440/149652425-a862341d-8d0e-4989-a427-3a9d94eabe31.png";
pub(crate) const IMG_KIND_DOCX: &str = "https://user-images.githubusercontent.com/16791440/149652426-acd2f779-916a-4ed7-baf5-0a577de5567b.png";
pub(crate) const IMG_KIND_EPUB: &str = "https://user-images.githubusercontent.com/16791440/149652479-d5073617-a7ac-48c3-ad36-275e3d96daac.png";
pub(crate) const IMG_KIND_EXE: &str = "https://user-images.githubusercontent.com/16791440/149652481-a08f1a34-919b-4bbf-8549-1b6a76eb28b1.png";
pub(crate) const IMG_KIND_GIF: &str = "https://user-images.githubusercontent.com/16791440/149652482-fbfdbfe7-dd1b-4695-bcbb-18845a291aa1.png";
pub(crate) const IMG_KIND_HTML: &str = "https://user-images.githubusercontent.com/16791440/149652485-92807883-7842-483c-8dec-06edbe5a2c5a.png";
pub(crate) const IMG_KIND_EOF: &str = "https://user-images.githubusercontent.com/16791440/149652537-2d598094-0bf4-4d4b-95aa-31c921abf095.png";
pub(crate) const IMG_KIND_JPG: &str = "https://user-images.githubusercontent.com/16791440/149652546-f41b2f50-c4d0-4226-9fce-42955886f870.png";
pub(crate) const IMG_KIND_JS: &str = "https://user-images.githubusercontent.com/16791440/149652547-9c640745-b3d1-430b-9806-f45a1bb367d0.png";
pub(crate) const IMG_KIND_JSON: &str = "https://user-images.githubusercontent.com/16791440/149652550-3e3a47ec-8ec5-4d58-8114-a89b69169cde.png";
pub(crate) const IMG_KIND_M3U: &str = "https://user-images.githubusercontent.com/16791440/149652551-0381bf88-424f-4326-8b8a-e202d4dffd5a.png";
pub(crate) const IMG_KIND_MKV: &str = "https://user-images.githubusercontent.com/16791440/149652553-f116e50c-e3a4-407f-8dd3-ee3857cb9b23.png";
pub(crate) const IMG_KIND_MOBI: &str = "https://user-images.githubusercontent.com/16791440/149652555-a1cf7272-395c-41e5-8fb9-2003684d51d7.png";
pub(crate) const IMG_KIND_MP4: &str = "https://user-images.githubusercontent.com/16791440/149652556-6662c887-0bdd-4383-87f8-3bf424ae16be.png";
pub(crate) const IMG_KIND_PDF: &str = "https://user-images.githubusercontent.com/16791440/149652558-62212053-8c27-4115-9bca-fc837815280b.png";
pub(crate) const IMG_KIND_PNG: &str = "https://user-images.githubusercontent.com/16791440/149652559-a1995332-678c-4e93-b153-332ef66d5b7b.png";
pub(crate) const IMG_KIND_RAR: &str = "https://user-images.githubusercontent.com/16791440/149652615-2a3fed6d-a2ba-4150-b416-15f71d187b1a.png";
pub(crate) const IMG_KIND_SRT: &str = "https://user-images.githubusercontent.com/16791440/149652616-1704a6a5-9c7c-4dbe-9592-bc65b508ec93.png";
pub(crate) const IMG_KIND_SSA: &str = "https://user-images.githubusercontent.com/16791440/149652617-851d009b-bcf1-45d4-acc5-5643c97685be.png";
pub(crate) const IMG_KIND_TORRENT: &str = "https://user-images.githubusercontent.com/16791440/149652618-efc53a39-943c-4021-adb4-1752d22e8e59.png";
pub(crate) const IMG_KIND_TXT: &str = "https://user-images.githubusercontent.com/16791440/149652619-f9e8cb5f-69ba-42de-b56b-fa6919b08c4a.png";
pub(crate) const IMG_KIND_WMV: &str = "https://user-images.githubusercontent.com/16791440/149652620-ea8c0388-a5b2-4a64-a8ef-f4724d4b9e8c.png";
pub(crate) const IMG_KIND_XLS: &str = "https://user-images.githubusercontent.com/16791440/149652621-bdbf40d6-8df7-4ce0-a577-4aed11a2d911.png";
pub(crate) const IMG_KIND_XLSX: &str = "https://user-images.githubusercontent.com/16791440/149652622-c2f15613-2e34-407a-968c-b2cf6deacab7.png";
pub(crate) const IMG_KIND_YAML: &str = "https://user-images.githubusercontent.com/16791440/149652623-9b9d9a91-cc64-41d2-8ecf-93a0a1f7f85f.png";
pub(crate) const IMG_KIND_ZIP: &str = "https://user-images.githubusercontent.com/16791440/149652624-8fe48257-dc6f-43ea-b32f-a6319422b5de.png";

pub(crate) const IMG_KIND_FILE_GIF: &str = "https://user-images.githubusercontent.com/16791440/149652484-fee48531-4f5f-4687-a782-3ec25cc481c9.png";
pub(crate) const IMG_KIND_FILE_MUSIC:&str = "https://user-images.githubusercontent.com/16791440/149652692-a191c169-cc3e-4889-93e0-2d8314291ca7.png";
pub(crate) const IMG_KIND_NORMAL:&str = "https://user-images.githubusercontent.com/16791440/149652695-bea5c55f-9e59-4463-97b4-8f73b639f9eb.png";
pub(crate) const IMG_KIND_FILE_OTHERS:&str = "https://user-images.githubusercontent.com/16791440/149652696-4a1d30bf-d9bd-4c8c-b550-227d8c578cd5.png";
pub(crate) const IMG_KIND_FILE_PICTURE:&str = "https://user-images.githubusercontent.com/16791440/149652697-c2ad5221-08f7-41f2-b0e1-b1f505d756d2.png";
pub(crate) const IMG_KIND_FILE_STICKERS:&str = "https://user-images.githubusercontent.com/16791440/149652698-efa915db-37ed-45fc-846c-7143707bdad4.png";
pub(crate) const IMG_KIND_FILE_VIDEO:&str = "https://user-images.githubusercontent.com/16791440/149652699-8b3d0a0b-6b4a-4ccb-acb4-5289002f49d5.png";