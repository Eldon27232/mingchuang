//! 常见扩展名预设 — 用户在 UI 上勾选,后端按预设展开

use crate::fileassoc::AssocPreset;

pub fn all() -> Vec<AssocPreset> {
    vec![
        AssocPreset {
            id: "music".into(),
            label: "音乐".into(),
            extensions: vec![
                ".mp3", ".flac", ".wav", ".m4a", ".aac", ".ape", ".ogg", ".wma", ".opus", ".aiff",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        AssocPreset {
            id: "video".into(),
            label: "视频".into(),
            extensions: vec![
                ".mp4", ".mkv", ".avi", ".mov", ".wmv", ".flv", ".webm", ".ts", ".m2ts", ".mts",
                ".rmvb", ".rm", ".m4v", ".mpg", ".mpeg", ".3gp",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        AssocPreset {
            id: "archive".into(),
            label: "压缩包".into(),
            extensions: vec![
                ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".zst", ".tgz", ".cab",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        AssocPreset {
            id: "image".into(),
            label: "图片".into(),
            extensions: vec![
                ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp", ".tiff", ".tif", ".avif", ".ico",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        AssocPreset {
            id: "doc".into(),
            label: "文档".into(),
            extensions: vec![".pdf", ".txt", ".log", ".md", ".rtf", ".csv"]
                .into_iter()
                .map(String::from)
                .collect(),
        },
    ]
}
