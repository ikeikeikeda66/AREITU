use std::path::PathBuf;

use anyhow::Result;
use areitu_core::{
    calendar::ingest_calendar_file,
    db,
    pipeline::build_visits,
    query::{list_places, visits_of, SortBy},
    resolve::{
        geocode::Nominatim,
        llm::{LlmClient, Ollama},
        Resolver,
    },
    scan::scan_photos,
    store::rename_place,
};
use clap::{Parser, Subcommand, ValueEnum};

const USER_AGENT: &str = concat!(
    "AREITU/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ikeikeikeda66/AREITU)"
);

#[derive(Parser)]
#[command(name = "areitu", version, about = "あれ、いつ行ったっけ？ 訪問ログ検証用 CLI")]
struct Cli {
    #[arg(long, global = true, default_value = "areitu.db")]
    db: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 写真フォルダを走査して Exif を取り込む
    IngestPhotos { dir: PathBuf },
    /// Google Calendar events.list の JSON を取り込む
    IngestCalendar { file: PathBuf },
    /// 未処理ログから訪問を作り、店舗名を推論する
    Build {
        #[arg(long)]
        ollama_model: Option<String>,
        #[arg(long, default_value = "http://localhost:11434")]
        ollama_url: String,
        #[arg(long, default_value_t = 0.6)]
        min_confidence: f64,
    },
    /// 場所の一覧（検索・並び替え）
    List {
        #[arg(long, value_enum, default_value_t = Sort::Count)]
        sort: Sort,
        #[arg(long)]
        search: Option<String>,
    },
    /// 場所の訪問日時一覧
    Show { place_id: i64 },
    /// 場所名を修正し、ユーザー辞書に記録する
    Rename { place_id: i64, name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    Count,
    Recent,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut conn = db::open(&cli.db)?;
    match cli.cmd {
        Cmd::IngestPhotos { dir } => {
            let r = scan_photos(&conn, &dir)?;
            println!("写真 {} 枚を確認、{} 件取り込み、{} 件スキップ", r.seen, r.inserted, r.skipped);
        }
        Cmd::IngestCalendar { file } => {
            println!("予定 {} 件を取り込みました", ingest_calendar_file(&conn, &file)?);
        }
        Cmd::Build { ollama_model, ollama_url, min_confidence } => {
            let geocoder = Nominatim::new(USER_AGENT)?;
            let ollama = ollama_model.map(|m| Ollama::new(&ollama_url, &m)).transpose()?;
            let resolver = Resolver {
                geocoder: &geocoder,
                llm: ollama.as_ref().map(|o| o as &dyn LlmClient),
                min_confidence,
            };
            let r = build_visits(&mut conn, &resolver)?;
            println!("訪問 {} 件を作成、{} 件失敗（次回再試行）", r.visits, r.failed);
            for e in r.errors {
                eprintln!("  {e}");
            }
        }
        Cmd::List { sort, search } => {
            let sort = match sort {
                Sort::Count => SortBy::Count,
                Sort::Recent => SortBy::Recent,
            };
            let places = list_places(&conn, sort, search.as_deref())?;
            if places.is_empty() {
                println!("場所はまだありません");
            }
            for p in places {
                println!("{}\t{}\t{} 回\t最終 {}", p.id, p.name, p.visit_count, p.last_visit.format("%Y-%m-%d %H:%M"));
            }
        }
        Cmd::Show { place_id } => {
            for (start, end) in visits_of(&conn, place_id)? {
                println!("{} 〜 {}", start.format("%Y-%m-%d %H:%M"), end.format("%H:%M"));
            }
        }
        Cmd::Rename { place_id, name } => {
            let id = rename_place(&mut conn, place_id, &name)?;
            println!("場所 {id} の名前を「{}」にしました", name.trim());
        }
    }
    Ok(())
}
