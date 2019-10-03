use clap::ArgMatches;
use kroeg_cellar::{CellarConnection, CellarEntityStore};
use kroeg_server::{config::Config, context, store::RetrievingEntityStore};
use kroeg_tap::{EntityStore, StoreItem};
use serde_json::Value;

async fn print_entity(config: &Config, value: Value, format: &str) {
    match format {
        "expand" => println!("{}", value),
        "compact" => {
            let compacted = context::compact(&config.server.base_uri, &value)
                .await
                .unwrap();
            println!("{}", compacted);
        }

        _ => unreachable!(),
    }
}

async fn get(config: &Config, store: &mut dyn EntityStore, id: String, local: bool, format: &str) {
    if let Some(entity) = store.get(id, local).await.expect("failed to get entity") {
        let entity = entity.to_json();
        print_entity(config, entity, format).await;
    }
}

async fn set(config: &Config, store: &mut dyn EntityStore, id: String, format: &str) {
    let data = serde_json::from_reader(std::io::stdin()).unwrap();
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

pub async fn handle(config: Config, matches: &ArgMatches<'_>) {
    let is_remote = matches.is_present("remote");
    let format = matches.value_of("format").unwrap();
    let id = matches.value_of("ID").unwrap();

    let database = CellarConnection::connect(
        &config.database.server,
        &config.database.username,
        &config.database.password,
        &config.database.database,
    )
    .await
    .expect("Database connection failed");
    let mut entitystore = RetrievingEntityStore::new(
        CellarEntityStore::new(&database),
        config.server.base_uri.to_owned(),
    );

    match matches.subcommand() {
        ("get", _) => get(&config, &mut entitystore, id.to_owned(), !is_remote, format).await,
        ("set", _) => set(&config, &mut entitystore, id.to_owned(), format).await,
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
