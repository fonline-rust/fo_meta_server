use std::path::{Path, PathBuf};
use fo_meta_server::*;

fn main() -> std::io::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Warn)
        .filter(Some("actix_web"), log::LevelFilter::Info)
        .filter(Some("actix_server"), log::LevelFilter::Info)
        //.filter(Some("serenity"), log::LevelFilter::Info)
        .init();
    let config = config::setup().expect("config.toml file");
    //println!("{:?}", config);

    let items = fo_proto_format::build_btree(&config.paths.proto_items);

    let fo_data = fo_data::FoData::init(&config.paths.game_client, &config.paths.palette)
        .expect("FoData loading");
    println!(
        "FoData loaded, archives: {}, files: {}",
        fo_data.retriever.data().count_archives(),
        fo_data.retriever.data().count_files()
    );

    let mut db_path = PathBuf::new();
    db_path.push("db");
    db_path.push("sled");
    let db = sled::open(db_path).expect("Can't open sled database");

    let state = web::AppDefinition::new(config, db).with_data(fo_data).with_items(items).build();
    web::run(state);
    //db.flush().expect("Can't flush sled database");
    //join_handle.join().unwrap();
    Ok(())
}
