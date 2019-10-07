use crate::config;
use clap::ArgMatches;
use kroeg_server::{
    config::ServerConfig, context, store::RetrievingEntityStore, LeasedConnection, StorePool,
};
use kroeg_tap::{EntityStore, StoreItem};
use serde_json::Value;
use std::io::{stdin, BufRead};

async fn print_entity(config: &ServerConfig, value: Value, format: &str) {
    match format {
        "expand" => println!("{}", value),
        "compact" => {
            let compacted = context::compact(&config.domain, &value).await.unwrap();
            println!("{}", compacted);
        }

        _ => unreachable!(),
    }
}

async fn get(
    config: &ServerConfig,
    store: &mut dyn EntityStore,
    id: String,
    local: bool,
    format: &str,
) {
    if let Some(entity) = store.get(id, local).await.expect("failed to get entity") {
        let entity = entity.to_json();
        print_entity(config, entity, format).await;
    }
}

async fn set(config: &ServerConfig, store: &mut dyn EntityStore, id: String, format: &str) {
    let data = serde_json::from_reader(stdin()).unwrap();
    let expanded = jsonld::expand::<context::SurfContextLoader>(
        &context::apply_supplement(data),
        &jsonld::JsonLdOptions {
            base: None,
            compact_arrays: None,
            expand_context: None,
            processing_mode: None,
        },
    )
    .await
    .expect("Failed to expand");

    let mut item = StoreItem::parse(&id, &expanded).expect("Failed to parse as store item");
    store
        .put(id, &mut item)
        .await
        .expect("failed to put entity");

    let entity = item.to_json();
    print_entity(config, entity, format).await;
}

async fn list(store: &mut dyn EntityStore, id: String) {
    let items = store
        .read_collection(id, Some(i32::max_value() as u32), None)
        .await
        .expect("failed to get");
    for item in items.items {
        println!("{}", item);
    }
}

async fn add(store: &mut dyn EntityStore, id: String, item: String) {
    store
        .insert_collection(id, item)
        .await
        .expect("failed to insert to collection");
}

async fn del(store: &mut dyn EntityStore, id: String, item: String) {
    store
        .remove_collection(id, item)
        .await
        .expect("failed to remove from collection");
}

pub async fn handle(config: config::KroegConfig, matches: &ArgMatches<'_>) {
    let is_remote = matches.is_present("remote");
    let format = matches.value_of("format").unwrap();
    let id = matches.value_of("ID").unwrap();
    let pool = crate::DatabasePool(config.database);
    let mut conn = pool.connect().await.expect("Database connection failed");

    let mut entitystore = RetrievingEntityStore::new(conn.get().0, config.server.domain.to_owned());

    match matches.subcommand() {
        ("get", _) => {
            get(
                &config.server,
                &mut entitystore,
                id.to_owned(),
                !is_remote,
                format,
            )
            .await
        }
        ("set", _) => set(&config.server, &mut entitystore, id.to_owned(), format).await,
        ("list", _) => list(&mut entitystore, id.to_owned()).await,
        ("add", cmd) => {
            add(
                &mut entitystore,
                id.to_owned(),
                cmd.unwrap().value_of("ID").unwrap().to_owned(),
            )
            .await
        }
        ("del", cmd) => {
            del(
                &mut entitystore,
                id.to_owned(),
                cmd.unwrap().value_of("ID").unwrap().to_owned(),
            )
            .await
        }
        _ => unreachable!(),
    }
}

pub async fn handle_query(config: config::KroegConfig) {
    let pool = crate::DatabasePool(config.database);
    let mut conn = pool.connect().await.expect("Database connection failed");

    let (store, _) = conn.get();

    let mut query_lines = Vec::new();
    for line in stdin().lock().lines() {
        let line = line.unwrap();

        query_lines.push(line.parse().unwrap());
    }

    let queried = store
        .query(query_lines)
        .await
        .expect("Database request failed");
    for item in queried {
        for item in item {
            print!("{}\t", item);
        }

        println!();
    }
}
