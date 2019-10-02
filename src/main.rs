use http::Response;
use http_service::Body;
use kroeg_server::{
    config, context, get, launch_delivery, nodeinfo, post, router::RequestHandler, router::Route,
    webfinger, KroegService, ServerError,
};
use kroeg_tap::Context;

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

fn listen_future(address: &str, config: &config::Config) {
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
    let config = config::read_config();

    println!("Kroeg v{} starting...", env!("CARGO_PKG_VERSION"));

    for _ in 0..config.deliver.unwrap_or(0) {
        async_std::task::spawn(launch_delivery(config.clone()));
    }

    if let Some(ref address) = config.listen {
        listen_future(address, &config);
    }
}
