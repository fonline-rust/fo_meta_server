use std::path::PathBuf;

use fo_meta_server::{config, log, sled, web};

fn main() -> std::io::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Warn)
        .filter(Some("actix_web"), log::LevelFilter::Info)
        .filter(Some("actix_server"), log::LevelFilter::Info)
        //.filter(Some("serenity"), log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let config = config::setup().expect("config.toml file");
    //println!("{:?}", config);

    let mut db_path = PathBuf::new();
    db_path.push("db");
    db_path.push("sled");
    let db = sled::open(db_path).expect("Can't open sled database");

    let state = web::AppDefinition::new(config, db).build();
    web::run(state);
    //db.flush().expect("Can't flush sled database");
    Ok(())
}
