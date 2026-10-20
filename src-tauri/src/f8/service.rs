//! Protocol-2 single-writer service. Construction requires an already validated V8 engine;
//! this module never opens a database, migrates an installation or launches a provider itself.
use super::{
    authority::{Authority, Client},
    engine::Spawner,
};
use crate::contracts::control::ControlResult;
use std::time::{Duration, Instant};

/// The application remains on protocol 1 until a separately authorized maintenance operation
/// supplies an F8 service. No environment variable or remote request can enable migration.
pub const INSTALLED_F8_ENABLED: bool = false;
// Compile-time guard: flipping the activation flag fails the build, not a test.
const _: () = assert!(!INSTALLED_F8_ENABLED);
/// Cooperative supervisor cadence; no reaper/worker busy loop when the service is idle.
pub const SUPERVISOR_INTERVAL: Duration = Duration::from_millis(25);

pub struct Service<S: Spawner> {
    authority: Authority,
    spawner: S,
}
impl<S: Spawner> Service<S> {
    pub fn new(authority: Authority, spawner: S) -> Self {
        Self { authority, spawner }
    }
    pub fn authority(&self) -> &Authority {
        &self.authority
    }
    pub fn authority_mut(&mut self) -> &mut Authority {
        &mut self.authority
    }
    pub fn exchange(&mut self, client: &mut Client, bytes: &[u8], now: Instant) -> Vec<u8> {
        self.authority.handle(client, bytes, &mut self.spawner, now)
    }
    pub fn tick(&mut self, now: Instant) -> ControlResult<()> {
        self.authority.tick(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::{WireUuid, control::ControlError},
        control::{F8Transport, f8_fixture},
        f8::{
            engine::{Engine, Spawned},
            migration,
            protocol::RuntimeRef,
            registry::Registry,
            staging::LaunchStaging,
        },
    };
    use serde_json::{Value, json};
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };
    struct NoSpawn;
    impl Spawner for NoSpawn {
        fn spawn(
            &mut self,
            _: &RuntimeRef,
            _: Option<&mut LaunchStaging>,
        ) -> ControlResult<Spawned> {
            Err(ControlError::Unsupported)
        }
    }
    fn transport() -> F8Transport<NoSpawn> {
        let mut source = f8_fixture::v7_with_completed_turn();
        let conn = migration::prepare(&mut source).unwrap().into_connection();
        let auth = WireUuid::try_from("00000000-0000-4000-8000-0000000000a1".to_owned()).unwrap();
        let engine = Engine::new(conn, auth.clone(), Registry::new(auth).unwrap(), None).unwrap();
        F8Transport::new(Service::new(Authority::new(engine), NoSpawn))
    }
    #[test]
    fn actual_control_transport_negotiates_two_and_rejects_v1_without_fallback() {
        for version in [1, 2] {
            let mut server = transport();
            let (owned, mut client) = UnixStream::pair().unwrap();
            client.set_nonblocking(true).unwrap();
            let now = Instant::now();
            server.attach(owned, now).unwrap();
            let request = serde_json::to_vec(&json!({"schema_version":version,
                "request_id":"00000000-0000-4000-8000-000000000001","method":"authority/hello",
                "params":{"protocol_version":version,"client_instance_id":"00000000-0000-4000-8000-000000000002"}})).unwrap();
            client
                .write_all(&(request.len() as u32).to_be_bytes())
                .unwrap();
            client.write_all(&request).unwrap();
            for _ in 0..3 {
                server.poll(now).unwrap();
            }
            let mut header = [0u8; 4];
            client.read_exact(&mut header).unwrap();
            let mut answer = vec![0; u32::from_be_bytes(header) as usize];
            client.read_exact(&mut answer).unwrap();
            let answer: Value = serde_json::from_slice(&answer).unwrap();
            if version == 1 {
                assert_eq!(answer["error"]["code"], "protocol_version_unsupported");
            } else {
                assert_eq!(answer["result"]["value"]["protocol_version"], 2);
            }
        }
    }
    #[test]
    fn clients_and_partial_frames_are_bounded() {
        let mut server = transport();
        let now = Instant::now();
        let mut clients = Vec::new();
        for _ in 0..8 {
            let (owned, mut client) = UnixStream::pair().unwrap();
            server.attach(owned, now).unwrap();
            client.write_all(&[0, 1]).unwrap();
            clients.push(client);
        }
        let (owned, _client) = UnixStream::pair().unwrap();
        assert_eq!(
            server.attach(owned, now),
            Err(ControlError::CapacityExceeded)
        );
        server.poll(now).unwrap();
        assert_eq!(server.client_count(), 8);
        server.poll(now + Duration::from_secs(4)).unwrap();
        assert_eq!(server.client_count(), 0);
    }
}
