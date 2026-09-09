//! Reads live Microsoft Teams presence status from `teams-for-linux`'s
//! built-in MQTT publisher (https://ismaelmartinez.github.io/teams-for-linux/mqtt-integration/),
//! so this app can react to status changes (e.g. flash the lights red when
//! you go "Busy") without any OAuth/Azure app registration - it just reads
//! whatever the already-logged-in Teams client is already publishing to a
//! local MQTT broker.
//!
//! Requires:
//! - A local MQTT broker (e.g. Mosquitto) listening on 127.0.0.1:1883.
//! - teams-for-linux configured with `"mqtt": {"enabled": true, "brokerUrl":
//!   "mqtt://127.0.0.1:1883", ...}` in its `config.json` (NOT `settings.json`
//!   - that's a different, internal file).
//!
//! Known status values observed from a real client: "available", "busy",
//! "do_not_disturb", "away". Others may exist (e.g. "be_right_back") but
//! weren't seen in testing - this is treated as an open set (unmapped
//! values are just ignored, not an error).

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};
use serde::Deserialize;
use std::sync::mpsc::Sender;
use std::time::Duration;

const BROKER_HOST: &str = "127.0.0.1";
const BROKER_PORT: u16 = 1883;
const STATUS_TOPIC: &str = "teams/status";

#[derive(Deserialize)]
struct StatusPayload {
    status: String,
}

/// Blocks forever, forwarding each distinct status string received. Meant to
/// be run in its own thread for the lifetime of the app - if the broker
/// isn't reachable this just returns immediately (nothing to retry against
/// yet; the UI shows the resulting error once and the user can toggle the
/// integration off/on to retry).
pub fn run_watcher(tx: Sender<String>) {
    let mut options = MqttOptions::new("ir-blaster-teams-watcher", BROKER_HOST, BROKER_PORT);
    options.set_keep_alive(Duration::from_secs(30));

    let (client, mut connection) = Client::new(options, 10);
    if client.subscribe(STATUS_TOPIC, QoS::AtMostOnce).is_err() {
        return;
    }

    for notification in connection.iter() {
        let Ok(Event::Incoming(Packet::Publish(publish))) = notification else {
            continue;
        };
        if let Ok(payload) = serde_json::from_slice::<StatusPayload>(&publish.payload) {
            if tx.send(payload.status).is_err() {
                return; // receiver (the App) is gone - nothing left to do
            }
        }
    }
}
