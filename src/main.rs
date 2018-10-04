extern crate dotenv;
extern crate futures;
extern crate hyper;
extern crate kroeg_server;

#[cfg(feature = "mastodon")]
extern crate kroeg_mastodon;

#[cfg(feature = "oauth")]
extern crate kroeg_oauth;

use futures::{future, Future};
use hyper::{Body, Response, Server};
use kroeg_server::{
    compact_response, config, context, get, launch_delivery, post, router::Route, webfinger,
    KroegServiceBuilder,
};

fn listen_future(
    address: &str,
    config: &config::Config,
) -> impl Future<Item = (), Error = ()> + Send + 'static {
    let addr = address.parse().expect("Invalid listen address!");

    let routes = vec![
        Route::get_prefix("/", compact_response(get::get)),
        Route::post_prefix("/", compact_response(post::post)),
        Route::get(
            "/-/context",
            Box::new(|_, store, queue, _| {
                Box::new(future::ok((
                    store,
                    queue,
                    Response::builder()
                        .status(200)
                        .header("Content-Type", "application/ld+json")
                        .body(Body::from(context::read_context().to_string()))
                        .unwrap(),
                )))
            }),
        ),
    ];

    let mut builder = KroegServiceBuilder {
        config: config.clone(),
        routes: routes,
    };

    webfinger::register(&mut builder);

    #[cfg(feature = "mastodon")]
    kroeg_mastodon::register(&mut builder);

    #[cfg(feature = "oauth")]
    kroeg_oauth::register(&mut builder);

    println!(" [+] listening at {}", addr);

    Server::bind(&addr).serve(builder).map_err(|_| ())
}

fn main() {
    dotenv::dotenv().ok();
    let config = config::read_config();

    println!("Kroeg v{} starting...", env!("CARGO_PKG_VERSION"));

    hyper::rt::run(hyper::rt::lazy(move || {
        if let Some(ref address) = config.listen {
            hyper::rt::spawn(listen_future(address, &config));
        }

        for _ in 0..config.deliver.unwrap_or(0) {
            hyper::rt::spawn(launch_delivery(config.clone()));
        }

        Ok(())
    }))
}
