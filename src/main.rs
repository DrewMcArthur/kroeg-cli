use clap::{App, AppSettings, Arg, SubCommand};
use http::Response;
use http_service::Body;
use kroeg_server::{
    config, context, get, launch_delivery, nodeinfo, post, router::RequestHandler, router::Route,
    webfinger, KroegService, ServerError,
};
use kroeg_tap::Context;

mod entity;
mod request;

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

fn listen(address: &str, config: &config::Config) {
    let addr = address.parse().expect("Invalid listen address!");

    let mut routes = vec![
        Route::get_prefix("/", get::GetHandler),
        Route::post_prefix("/", post::PostHandler),
        Route::get("/-/context", ContextHandler),
    ];

    routes.append(&mut nodeinfo::routes());
    routes.append(&mut webfinger::routes());

    #[cfg(feature = "mastodon")]
    routes.append(&mut kroeg_mastodon::routes());

    #[cfg(feature = "oauth")]
    routes.append(&mut kroeg_oauth::routes());

    let builder = KroegService::new(config.clone(), routes);

    println!("Listening at: {}", addr);
    http_service_hyper::run(builder, addr);
}

fn main() {
    dotenv::dotenv().ok();

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
        .get_matches();

    let config = config::read_config(matches.value_of("config").unwrap_or("server.toml"));
    match matches.subcommand() {
        ("entity", Some(subcommand)) => {
            async_std::task::block_on(entity::handle(config, subcommand))
        }
        ("request", Some(subcommand)) => {
            async_std::task::block_on(request::handle(config, subcommand))
        }
        ("serve", Some(subcommand)) => {
            let queue: usize = subcommand.value_of("queue").unwrap_or("0").parse().unwrap();
            let address = subcommand.value_of("ADDRESS").unwrap_or("");

            let extra_count = if !address.is_empty() || queue == 0 {
                queue
            } else {
                queue - 1
            };
            for _ in 0..extra_count {
                async_std::task::spawn(launch_delivery(config.clone()));
            }

            if !address.is_empty() {
                listen(address, &config);
            } else if queue > 0 {
                async_std::task::block_on(launch_delivery(config));
            }
        }
        _ => unreachable!(),
    }
}
