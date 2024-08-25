// Copyright 2020 - developers of the `grammers` project.
// Copyright 2021 - developers of the `tdlib-rs` project.
// Copyright 2024 - developers of the `tgt` and `tdlib-rs` projects.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
pub mod build;
mod generated;
mod observer;
mod tdjson;

pub use generated::{enums, functions, types};

use enums::Update;
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::{sync::oneshot::error::TryRecvError, time::sleep};
use std::{sync::atomic::{AtomicU32, Ordering}, time::Duration};
use regex::Regex;

static EXTRA_COUNTER: AtomicU32 = AtomicU32::new(0);
static OBSERVER: Lazy<observer::Observer> = Lazy::new(observer::Observer::new);

/// Create a TdLib client returning its id. Note that to start receiving
/// updates for a client you need to send at least a request with it first.
pub fn create_client() -> i32 {
    tdjson::create_client()
}

/// Receive a single update or response from TdLib. If it's an update, it
/// returns a tuple with the `Update` and the associated `client_id`.
/// Note that to start receiving updates for a client you need to send
/// at least a request with it first.
pub fn receive() -> Option<(Update, i32)> {
    let response = tdjson::receive(2.0);
    if let Some(response_str) = response {
        let response: Value = serde_json::from_str(&response_str).unwrap();

        match response.get("@extra") {
            Some(_) => {
                OBSERVER.notify(response);
            }
            None => {
                let client_id = response["@client_id"].as_i64().unwrap() as i32;
                match serde_json::from_value(response) {
                    Ok(update) => {
                        return Some((update, client_id));
                    }
                    Err(e) => {
                        log::warn!(
                            "Received an unknown response: {}\nReason: {}",
                            response_str,
                            e
                        );
                    }
                }
            }
        }
    }

    None
}

static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"retry after (\d+)").unwrap());

pub(crate) async fn send_request(client_id: i32, mut request: Value) -> Value {
    loop {
        let extra = EXTRA_COUNTER.fetch_add(1, Ordering::Relaxed);
        request["@extra"] = serde_json::to_value(extra).unwrap();

        let mut receiver = OBSERVER.subscribe(extra);
        tdjson::send(client_id, request.to_string());

        loop {
            match receiver.try_recv() {
                Ok(v) => {
                    // println!("req{:?} res{:?}",request,v);
                    if v["code"].as_i64() == Some(429) {
                        if let Some(message_reason) = v["message"].as_str() {
                            if let Some(captures) = RE.captures(message_reason) {
                                if let Some(second_str) = captures.get(1) {
                                    let seconds = second_str.as_str().parse().unwrap();
                                    println!("Wait for {} seconds", seconds);
                                    sleep(Duration::from_secs(seconds)).await;
                                    break;
                                }
                            }
                        }
                    }
                    return v
                }
                Err(TryRecvError::Empty) => {
                    sleep(Duration::from_millis(10)).await;
                }
                Err(TryRecvError::Closed) => {
                    panic!("Closed");
                }
            }
        }
    }
}
