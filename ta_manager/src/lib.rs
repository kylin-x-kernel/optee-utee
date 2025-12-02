use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

use bincode::config;
use optee_utee::Result;

use crate::protocol::{CARequest, CAResponse, Parameters, TARequest};

const SERVER_SOCKET_PATH: &str = "/tmp/server.sock";

pub mod protocol;

/// Trait representing a Trusted Application (TA).
pub trait TrustedApplication {
    /// User-defined session context type.
    type SessionContext;

    /// Create a new TA instance.
    fn create(&self) -> Result<()>;

    /// Open a session with the TA.
    fn open_session(&self, params: &mut Parameters) -> Result<Self::SessionContext>;

    /// Close the session with the TA.
    fn close_session(&self, ctx: &mut Self::SessionContext) -> Result<()>;

    /// Destroy the TA instance.
    fn destroy(&self) -> Result<()>;

    /// Invoke a command on the TA.
    fn invoke_command(
        &self,
        cmd_id: u32,
        params: &mut Parameters,
        ctx: &mut Self::SessionContext,
    ) -> Result<()>;
}

pub struct TAManager<T: TrustedApplication> {
    uuid: String,
    sessions: HashMap<u32, T::SessionContext>,
    session_id: AtomicU32,
}

impl<T: TrustedApplication> TAManager<T> {
    pub fn new(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            sessions: HashMap::new(),
            session_id: AtomicU32::new(1),
        }
    }

    pub fn run_ta(&mut self, ta: &T) -> anyhow::Result<()> {
        ta.create()?;
        let _stream = self.register_ta()?;
        self.handle_ca_request(ta)?;
        Ok(())
    }

    // Register the TA with the TA Manager.
    fn register_ta(&self) -> anyhow::Result<UnixStream> {
        let mut stream = UnixStream::connect(SERVER_SOCKET_PATH)?;

        let req = TARequest::Register {
            uuid: self.uuid.clone(),
        };
        let data = bincode::encode_to_vec(req, config::standard())?;
        stream.write_all(&data)?;
        println!("TA registered with UUID: {}", self.uuid);

        Ok(stream)
    }

    // Handle requests from the Client Application (CA).
    fn handle_ca_request(&mut self, ta: &T) -> anyhow::Result<()> {
        let path = PathBuf::from(format!("/tmp/{}.sock", self.uuid));
        let _ = std::fs::remove_file(path.clone());

        let listener = UnixListener::bind(path.clone())?;
        println!("TA listening on socket: {:?}", path);

        for stream in listener.incoming() {
            println!("Received connection from CA");
            let mut stream = stream?;
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf)?;

            let (req, _): (CARequest, _) = bincode::decode_from_slice(&buf, config::standard())?;
            match req {
                CARequest::OpenSession { mut params } => {
                    let session_id = self.next_session_id();
                    println!("Opening session with ID: {}", session_id);
                    let resp = match ta.open_session(&mut params) {
                        Ok(ctx) => {
                            self.sessions.insert(session_id, ctx);
                            println!("Session {} opened successfully", session_id);
                            CAResponse::OpenSession {
                                status: 0,
                                session_id,
                            }
                        }
                        Err(e) => {
                            println!("Failed to open session {}: {:?}", session_id, e);
                            CAResponse::OpenSession {
                                status: e.raw_code(),
                                session_id: 0,
                            }
                        }
                    };
                    let resp_data = bincode::encode_to_vec(resp, config::standard())?;
                    stream.write_all(&resp_data)?;
                }
                CARequest::CloseSession { session_id } => {
                    let resp = match ta.close_session(self.sessions.get_mut(&session_id).unwrap()) {
                        Ok(_) => {
                            self.sessions.remove(&session_id);
                            println!("Session {} closed successfully", session_id);
                            CAResponse::CloseSession {
                                status: 0,
                                session_id,
                            }
                        }
                        Err(e) => {
                            println!("Failed to close session {}: {:?}", session_id, e);
                            CAResponse::CloseSession {
                                status: e.raw_code(),
                                session_id,
                            }
                        }
                    };
                    let resp_data = bincode::encode_to_vec(resp, config::standard())?;
                    stream.write_all(&resp_data)?;
                }
                CARequest::Destroy => {
                    ta.destroy()?;
                    break;
                }
                CARequest::InvokeCommand {
                    session_id,
                    cmd_id,
                    mut params,
                } => {
                    let resp = match ta.invoke_command(
                        cmd_id,
                        &mut params,
                        self.sessions.get_mut(&session_id).unwrap(),
                    ) {
                        Ok(_) => {
                            println!(
                                "Command {} invoked successfully on session {}",
                                cmd_id, session_id
                            );
                            CAResponse::InvokeCommand {
                                status: 0,
                                session_id,
                                cmd_id,
                                params,
                            }
                        }
                        Err(e) => {
                            println!(
                                "Failed to invoke command {} on session {}: {:?}",
                                cmd_id, session_id, e
                            );
                            CAResponse::InvokeCommand {
                                status: e.raw_code(),
                                session_id,
                                cmd_id,
                                params,
                            }
                        }
                    };
                    let resp_data = bincode::encode_to_vec(resp, config::standard())?;
                    stream.write_all(&resp_data)?;
                }
            }
        }

        Ok(())
    }

    fn next_session_id(&self) -> u32 {
        self.session_id.fetch_add(1, Ordering::SeqCst)
    }
}
