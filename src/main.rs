use clap::{App, AppSettings, Arg, SubCommand};
use http::Response;
use http_service::Body;
use kroeg_cellar::{CellarConnection, CellarEntityStore};
use kroeg_server::{
    context, get, launch_delivery, nodeinfo, post, router::RequestHandler, router::Route,
    webfinger, KroegService, LeasedConnection, ServerError, StorePool,
};
use kroeg_tap::{Context, EntityStore, QueueStore, StoreError};
use std::fs::File;
use std::future::Future;
use std::io::Read;
use std::pin::Pin;

mod config;
mod entity;
mod request;
mod user;

struct ContextHandler;

#[async_trait::async_trait]
impl RequestHandler for ContextHandler {
    async fn run(
        &self,
        _: &mut Context<'_, '_>,
        _: http_service::Request,
    ) -> Result<http_service::Response, ServerError> {
        Ok(Response::builder()
            .status(200)
            .header("Content-Type", "application/ld+json")
            .body(Body::from(context::read_context().to_string()))
            .unwrap())
    }
}

fn listen(address: &str, config: &config::KroegConfig) {
    let addr = address.parse().expect("Invalid listen address!");

    let mut routes = vec![
        Route::get_prefix("/", get::GetHandler),
        Route::post_prefix("/", post::PostHandler),
    ];

    #[cfg(feature = "frontend")]
    routes.append(&mut kroeg_frontend::routes().expect("Failed to register frontend"));

    // Ensure GETs with the proper Accept get handled ActivityPub-first.
    routes.push(Route {
        content_type: vec![
            "application/activity+json".to_string(),
            "application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"".to_string(),
        ],
        method: http::Method::GET,
        path: String::new(),
        is_prefix: true,
        handler: Box::new(get::GetHandler),
    });

    routes.push(Route::get("/-/context", ContextHandler));
    routes.append(&mut nodeinfo::routes());
    routes.append(&mut webfinger::routes());

    #[cfg(feature = "mastodon")]
    routes.append(&mut kroeg_mastodon::routes());

    #[cfg(feature = "oauth")]
    routes.append(&mut kroeg_oauth::routes());

    let pool = DatabasePool(config.database.clone());
    let builder = KroegService::new(pool, config.server.clone(), routes);

    println!("Listening at: {}", addr);
    http_service_hyper::run(builder, addr);
}

// Using a raw pointer here to 100% ensure the connection outlives the EntityStore and QueueStore
//  that make use of it. It is an ugly hack.
enum DatabaseConnection {
    PostgreSQL(
        *mut CellarConnection,
        Option<(CellarEntityStore<'static>, CellarEntityStore<'static>)>,
    ),
}

unsafe impl Send for DatabaseConnection {}

impl LeasedConnection for DatabaseConnection {
    fn get(&mut self) -> (&mut dyn EntityStore, &mut dyn QueueStore) {
        match self {
            DatabaseConnection::PostgreSQL(_, Some((left, right))) => (left, right),

            _ => unreachable!(),
        }
    }
}

impl Drop for DatabaseConnection {
    fn drop(&mut self) {
        match self {
            DatabaseConnection::PostgreSQL(conn, data) => {
                *data = None;
                unsafe { Box::from_raw(*conn) };
            }
        }
    }
}

struct DatabasePool(config::DatabaseConfig);

impl StorePool for DatabasePool {
    type LeasedConnection = DatabaseConnection;

    fn connect(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Self::LeasedConnection, StoreError>> + Send + 'static>>
    {
        let cloned = self.0.clone();

        Box::pin(async move {
            match &cloned {
                config::DatabaseConfig::PostgreSQL {
                    server,
                    username,
                    password,
                    database,
                } => {
                    let connection =
                        CellarConnection::connect(server, username, password, database);
                    let conn = Box::into_raw(Box::new(connection.await?));

                    let left = CellarEntityStore::new(unsafe { &*conn });
                    let right = CellarEntityStore::new(unsafe { &*conn });

                    Ok(DatabaseConnection::PostgreSQL(conn, Some((left, right))))
                }
            }
        })
    }
}

fn main() {
    let matches = App::new("Kroeg")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Puck Meerburg <puck@puckipedia.com>")
        .about("An ActivityPub server")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(
            Arg::with_name("config")
                .long("config")
                .value_name("FILE")
                .help("Sets a custom config file")
                .takes_value(true),
        )
        .subcommand(SubCommand::with_name("query").about("Runs a query passed on stdin"))
        .subcommand(
            SubCommand::with_name("entity")
                .about("Manipulates the entity store backend")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .arg(
                    Arg::with_name("remote")
                        .long("remote")
                        .help("Request entities from the remote server if not found locally"),
                )
                .arg(
                    Arg::with_name("format")
                        .long("format")
                        .help("The format to output entities as")
                        .possible_values(&["expand", "compact"])
                        .default_value("expand"),
                )
                .arg(
                    Arg::with_name("ID")
                        .help("The ID of the entity to get")
                        .required(true)
                        .index(1),
                )
                .subcommand(
                    SubCommand::with_name("get").about("Gets an item from the entity store"),
                )
                .subcommand(
                    SubCommand::with_name("set")
                        .about("Stores an item into the entity store, reading from stdin"),
                )
                .subcommand(
                    SubCommand::with_name("list")
                        .about("Lists the IDs of the object stored in this collection"),
                )
                .subcommand(
                    SubCommand::with_name("add")
                        .about("Inserts an entity into this collection")
                        .arg(
                            Arg::with_name("ID")
                                .help("The ID of the entity to insert into the collection")
                                .required(true)
                                .index(1),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("del")
                        .about("Removes an entity from this collection")
                        .arg(
                            Arg::with_name("ID")
                                .help("The ID of the entity to remove from the collection")
                                .required(true)
                                .index(1),
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("request")
                .about("Simulates requests to the server")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .arg(
                    Arg::with_name("URL")
                        .help("The URL that will be requested")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::with_name("format")
                        .long("format")
                        .help("The format to output entities as")
                        .possible_values(&["expand", "compact"])
                        .default_value("compact"),
                )
                .arg(
                    Arg::with_name("user")
                        .long("user")
                        .help("The user ID to use for the request")
                        .value_name("USER")
                        .takes_value(true),
                )
                .subcommand(SubCommand::with_name("get").about("Sends a GET request"))
                .subcommand(
                    SubCommand::with_name("post")
                        .about("Sends a POST request, with the body in stdin"),
                ),
        )
        .subcommand(
            SubCommand::with_name("serve")
                .about("Serves an HTTP server or one or more queue workers")
                .arg(
                    Arg::with_name("ADDRESS")
                        .help("The address to listen on")
                        .index(1),
                )
                .arg(
                    Arg::with_name("queue")
                        .help("The amount of queue workers to spin up")
                        .long("queue")
                        .value_name("COUNT")
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("actor")
                .about("Sets up and changes actors")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .arg(
                    Arg::with_name("ACTOR")
                        .help("The ID of the actor")
                        .index(1)
                        .required(true),
                )
                .subcommand(
                    SubCommand::with_name("create")
                        .about("Creates this actor")
                        .arg(
                            Arg::with_name("username")
                                .help("The preferredUsername of this user")
                                .long("username")
                                .value_name("NAME")
                                .takes_value(true),
                        )
                        .arg(
                            Arg::with_name("name")
                                .help("The 'full name' of this user")
                                .long("name")
                                .value_name("NAME")
                                .takes_value(true),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("token").about("Prints a bearer token for this actor"),
                ),
        )
        .get_matches();

    let config_filename = matches.value_of("config").unwrap_or("server.toml");
    let mut file = File::open(config_filename).unwrap();
    let mut data = Vec::new();
    file.read_to_end(&mut data).unwrap();
    let config: config::KroegConfig = toml::from_slice(&data).unwrap();

    match matches.subcommand() {
        ("entity", Some(subcommand)) => {
            async_std::task::block_on(entity::handle(config, subcommand))
        }
        ("query", _) => async_std::task::block_on(entity::handle_query(config)),
        ("request", Some(subcommand)) => {
            async_std::task::block_on(request::handle(config, subcommand))
        }
        ("actor", Some(subcommand)) => async_std::task::block_on(user::handle(config, subcommand)),
        ("serve", Some(subcommand)) => {
            let queue: usize = subcommand.value_of("queue").unwrap_or("0").parse().unwrap();
            let address = subcommand.value_of("ADDRESS").unwrap_or("");

            let extra_count = if !address.is_empty() || queue == 0 {
                queue
            } else {
                queue - 1
            };
            for _ in 0..extra_count {
                let pool = DatabasePool(config.database.clone());
                async_std::task::spawn(launch_delivery(pool, config.server.clone()));
            }

            if !address.is_empty() {
                listen(address, &config);
            } else if queue > 0 {
                let pool = DatabasePool(config.database);
                async_std::task::block_on(launch_delivery(pool, config.server));
            }
        }
        _ => unreachable!(),
    }
}
