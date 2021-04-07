#![feature(map_first_last)]
#![feature(result_contains_err)]
#![feature(destructuring_assignment)]

use std::net::IpAddr;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use clap::{App, Arg};
use tokio::sync::Mutex;
use warp::Filter;
use web3::types::Address;

#[macro_use]
extern crate enum_display_derive;

#[macro_use]
extern crate log;
extern crate pretty_env_logger;

pub mod book;
pub mod handler;
pub mod order;
pub mod rpc;
pub mod state;
pub mod tests;
pub mod util;

#[cfg(test)]
pub mod book_tests;

use crate::order::OrderId;
use crate::state::OmeState;

/// The default IP address that the OME will listen on
pub const DEFAULT_IP: &str = "0.0.0.0";

/// The default TCP port number that the OME will listen on
pub const DEFAULT_PORT: &str = "8989";

/// The default file path for reading and writing state dumps
pub const DEFAULT_DUMPFILE: &str = ".omedump.json";

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    /* define our command-line interface using Clap's builder syntax */
    let matches = App::new("Tracer OME")
        .version("0.1.0")
        .arg(
            Arg::with_name("address")
                .short("l")
                .long("listen")
                .value_name("address")
                .help("The address to listen on")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("port")
                .help("The TCP port to listen on")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("dumpfile")
                .long("dumpfile")
                .value_name("dumpfile")
                .help("The path to the dump file to use for state resumes")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("executioner_address")
                .long("executioner_address")
                .value_name("executioner_address")
                .help("Address of the Web3 executioner")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    /* determine what address to listen on - either the address the user
     * provided or the default address (see `DEFAULT_IP`) */
    let listen_address: IpAddr = match IpAddr::from_str(
        matches.value_of("address").unwrap_or(DEFAULT_IP),
    ) {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    /* determine what port number to listen on - either the port number the user
     * provided or the default port number (see `DEFAULT_PORT`) */
    let listen_port: u16 = match matches
        .value_of("port")
        .unwrap_or(DEFAULT_PORT)
        .parse::<u16>()
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let dumpfile_path: &Path =
        Path::new(matches.value_of("dumpfile").unwrap_or(DEFAULT_DUMPFILE));

    let executioner_address: String =
        matches.value_of("executioner_address").unwrap().to_owned();

    let internal_state = if util::is_existing_state(dumpfile_path) {
        match OmeState::from_dumpfile(dumpfile_path) {
            Some(s) => s,
            None => OmeState::new(),
        }
    } else {
        Default::default()
    };

    /* initialise engine state */
    let state: Arc<Mutex<OmeState>> = Arc::new(Mutex::new(internal_state));

    /* Clone global engine state for each handler. This is only done because of
     * the nature of move semantics for Rust closures.
     *
     * This is quite hacky and likely quite expensive.
     */
    let index_book_state: Arc<Mutex<OmeState>> = state.clone();
    let create_book_state: Arc<Mutex<OmeState>> = state.clone();
    let read_book_state: Arc<Mutex<OmeState>> = state.clone();

    let create_order_state: Arc<Mutex<OmeState>> = state.clone();
    let read_order_state: Arc<Mutex<OmeState>> = state.clone();
    let update_order_state: Arc<Mutex<OmeState>> = state.clone();
    let destroy_order_state: Arc<Mutex<OmeState>> = state.clone();

    /* define CRUD routes for order books */
    let book_prefix = warp::path!("book");
    let index_book_route = book_prefix
        .and(warp::get())
        .and(warp::any().map(move || index_book_state.clone()))
        .and_then(handler::index_book_handler);
    let create_book_route = book_prefix
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || create_book_state.clone()))
        .and_then(handler::create_book_handler);
    let read_book_route = warp::path!("book" / Address)
        .and(warp::get())
        .and(warp::any().map(move || read_book_state.clone()))
        .and_then(handler::read_book_handler);

    /* define CRUD routes for orders */
    let create_order_route = warp::path!("book" / Address / "order")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || create_order_state.clone()))
        .and(warp::any().map(move || executioner_address.clone()))
        .and_then(handler::create_order_handler);
    let read_order_route = warp::path!("book" / Address / "order" / OrderId)
        .and(warp::get())
        .and(warp::any().map(move || read_order_state.clone()))
        .and_then(handler::read_order_handler);
    let update_order_route = warp::path!("book" / Address / "order" / OrderId)
        .and(warp::put())
        .and(warp::body::json())
        .and(warp::any().map(move || update_order_state.clone()))
        .and_then(handler::update_order_handler);
    let destroy_order_route = warp::path!("book" / Address / "order" / OrderId)
        .and(warp::delete())
        .and(warp::any().map(move || destroy_order_state.clone()))
        .and_then(handler::destroy_order_handler);

    /* aggregate all of our order book routes */
    let book_routes =
        index_book_route.or(create_book_route).or(read_book_route);

    /* aggregate all of our order routes */
    let order_routes = create_order_route
        .or(read_order_route)
        .or(update_order_route)
        .or(destroy_order_route);

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE"]);

    /* aggregate all of our routes */
    let routes = book_routes.or(order_routes).with(cors);

    /* start the web server */
    warp::serve(routes).run((listen_address, listen_port)).await;
}