use crate::config::KroegConfig;
use clap::ArgMatches;
use http_service::Body;
use kroeg_server::{
    context, get, post, router::RequestHandler, store::RetrievingEntityStore, LeasedConnection,
    StorePool,
};
use kroeg_tap::{Context, User};
use std::collections::HashMap;
use std::io::{Read, Write};

async fn print_entity(value: Vec<u8>, format: &str) {
    match format {
        "expand" => {
            let data = serde_json::from_slice(&value).unwrap();

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

            println!("{}", expanded);
        }

        "compact" => {
            std::io::stdout().write_all(&value).unwrap();
        }

        _ => unreachable!(),
    }
}

pub async fn handle(config: KroegConfig, matches: &ArgMatches<'_>) {
    let format = matches.value_of("format").unwrap();
    let url = matches.value_of("URL").unwrap().to_owned();

    let pool = crate::DatabasePool(config.database);
    let mut conn = pool.connect().await.expect("Database connection failed");
    let (entity_store, queue_store) = conn.get();

    let mut entity_store =
        RetrievingEntityStore::new(entity_store, config.server.domain.to_owned());

    let mut context = Context {
        user: User {
            claims: HashMap::new(),
            issuer: Some("cli".to_owned()),
            subject: matches.value_of("user").unwrap_or("anonymous").to_owned(),
            audience: vec![],
            token_identifier: "cli".to_owned(),
        },

        server_base: config.server.domain.to_owned(),
        name: config.server.name.to_owned(),
        description: config.server.description.to_owned(),
        entity_store: &mut entity_store,
        queue_store,
        instance_id: config.server.instance_id,
    };

    let typ = matches.subcommand_name().unwrap();
    let body = if typ == "post" {
        let mut data = Vec::new();
        std::io::stdin().read_to_end(&mut data).unwrap();
        Body::from(data)
    } else {
        Body::from("")
    };

    let request = http::Request::builder()
        .uri(url)
        .method(typ)
        .body(body)
        .unwrap();

    let response = if typ == "post" {
        post::PostHandler.run(&mut context, request).await
    } else {
        get::GetHandler.run(&mut context, request).await
    }
    .unwrap();

    println!("HTTP/1.0 {}", response.status());
    for (k, v) in response.headers() {
        println!("{}: {}", k, v.to_str().unwrap());
    }

    println!();

    let body = response.into_body();

    print_entity(body.into_vec().await.unwrap(), format).await;
}
