use clap::ArgMatches;
use jsonld::nodemap::{Pointer, Value};
use kroeg_cellar::{CellarConnection, CellarEntityStore};
use kroeg_server::config::Config;
use kroeg_tap::{
    as2, kroeg, sec, untangle, Context, EntityStore, MessageHandler, QueueStore, StoreError, User,
};
use kroeg_tap_activitypub::handlers::CreateActorHandler;
use openssl::{hash::MessageDigest, pkey::PKey, rsa::Rsa, sign::Signer};
use serde_json::{json, Value as JValue};
use std::collections::HashMap;

async fn create_auth(store: &mut dyn EntityStore, id: String) -> Result<(), StoreError> {
    let person = store.get(id, false).await?.unwrap();

    let keyid = if let [Pointer::Id(id)] = &person.main()[sec!(publicKey)] as &[_] {
        id.to_owned()
    } else {
        eprintln!("Cannot create authentication for user: no key");
        return Ok(());
    };

    let mut key = store.get(keyid.to_owned(), false).await?.unwrap();
    let private = if let [Pointer::Value(Value {
        value: JValue::String(strval),
        ..
    })] = &key.meta()[sec!(privateKeyPem)] as &[_]
    {
        PKey::from_rsa(Rsa::private_key_from_pem(strval.as_bytes())?)?
    } else {
        eprintln!("Cannot create authentication for user: no private key");
        return Ok(());
    };

    let mut signer = Signer::new(MessageDigest::sha256(), &private).unwrap();
    let signed = format!(
        "{}.{}",
        base64::encode_config(
            json!({
                "typ": "JWT",
                "alg": "RS256",
                "kid": keyid
            })
            .to_string()
            .as_bytes(),
            base64::URL_SAFE_NO_PAD
        ),
        base64::encode_config(
            json!({
                "iss": "kroeg-call",
                "sub": person.id(),
                "exp": 0xFFFFFFFFu32
            })
            .to_string()
            .as_bytes(),
            base64::URL_SAFE_NO_PAD
        ),
    );

    signer.update(signed.as_bytes()).unwrap();
    let signature = base64::encode_config(&signer.sign_to_vec().unwrap(), base64::URL_SAFE_NO_PAD);

    println!("{}.{}", signed, signature);

    Ok(())
}

async fn create_actor(
    config: Config,
    store: &mut dyn EntityStore,
    queue: &mut dyn QueueStore,
    mut id: String,
    cmd: &ArgMatches<'_>,
) {
    let mut context = Context {
        user: User {
            claims: HashMap::new(),
            issuer: Some("cli".to_owned()),
            subject: "anonymous".to_owned(),
            audience: vec![],
            token_identifier: "cli".to_owned(),
        },

        server_base: config.server.base_uri.to_owned(),
        name: config.server.name.to_owned(),
        description: config.server.description.to_owned(),
        entity_store: store,
        queue_store: queue,
        instance_id: config.server.instance_id,
    };

    let mut json = json!({
        "@id": id,
        "@type": [as2!(Person)],
    });

    if let Some(item) = cmd.value_of("username") {
        json.as_object_mut().unwrap().insert(
            as2!(preferredUsername).to_owned(),
            json!([{ "@value": item }]),
        );
    }

    if let Some(item) = cmd.value_of("name") {
        json.as_object_mut()
            .unwrap()
            .insert(as2!(name).to_owned(), json!([{ "@value": item }]));
    }

    let untangled = untangle(&json).unwrap();
    for (key, mut value) in untangled {
        value.meta()[kroeg!(instance)].push(Pointer::Value(Value {
            value: context.instance_id.into(),
            type_id: Some("http://www.w3.org/2001/XMLSchema#integer".to_owned()),
            language: None,
        }));

        context.entity_store.put(key, &mut value).await.unwrap();
    }

    CreateActorHandler
        .handle(&mut context, &mut "".to_string(), &mut id)
        .await
        .unwrap();

    println!("done");
}

pub async fn handle(config: Config, matches: &ArgMatches<'_>) {
    let id = matches.value_of("ACTOR").unwrap().to_owned();

    let database = CellarConnection::connect(
        &config.database.server,
        &config.database.username,
        &config.database.password,
        &config.database.database,
    )
    .await
    .expect("Database connection failed");

    let mut entitystore = CellarEntityStore::new(&database);
    let mut queuestore = CellarEntityStore::new(&database);

    match matches.subcommand() {
        ("token", _) => create_auth(&mut entitystore, id).await.unwrap(),
        ("create", Some(cmd)) => {
            create_actor(config, &mut entitystore, &mut queuestore, id, cmd).await
        }
        _ => unreachable!(),
    }
}
