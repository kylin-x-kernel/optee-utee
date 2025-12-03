use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread,
};

use bincode::config;
use crossbeam_channel::{Receiver, Sender, unbounded};
use optee_utee::{ErrorKind, Result};

use crate::protocol::{CARequest, CAResponse, Parameters, TARequest};

const SERVER_SOCKET_PATH: &str = "/tmp/server.sock";

pub mod protocol;

/// Trait representing a Trusted Application (TA).
pub trait TrustedApplication: Send + Sync + 'static {
    /// User-defined session context type.
    type SessionContext: Send;

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
    ta: Arc<T>,
    uuid: String,
    sessions: HashMap<u32, Sender<SessionMessage>>,
    session_id: AtomicU32,
}

impl<T: TrustedApplication> TAManager<T> {
    pub fn new(ta: T, uuid: &str) -> Self {
        Self {
            ta: Arc::new(ta),
            uuid: uuid.to_string(),
            sessions: HashMap::new(),
            session_id: AtomicU32::new(1),
        }
    }

    pub fn run_ta(&mut self) -> anyhow::Result<()> {
        self.ta.create()?;
        let _stream = self.register_ta()?;
        self.handle_ca_request(self.ta.clone())?;
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
    fn handle_ca_request(&mut self, ta: Arc<T>) -> anyhow::Result<()> {
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
                CARequest::OpenSession { params } => {
                    self.handle_open_session(stream, ta.clone(), params)?
                }
                CARequest::CloseSession { session_id } => {
                    self.handle_close_session(stream, session_id)?
                }
                CARequest::Destroy => {
                    ta.destroy()?;
                    break;
                }
                CARequest::InvokeCommand {
                    session_id,
                    cmd_id,
                    params,
                } => self.handle_invoke_command(stream, session_id, cmd_id, params)?,
            }
        }

        Ok(())
    }

    fn handle_open_session(
        &mut self,
        mut stream: UnixStream,
        ta: Arc<T>,
        mut params: Parameters,
    ) -> anyhow::Result<()> {
        let session_id = self.next_session_id();
        println!("Opening session with ID: {}", session_id);

        let resp = match ta.open_session(&mut params) {
            Ok(ctx) => {
                println!("Session {} opened successfully", session_id);
                let (tx, rx) = unbounded();
                self.sessions.insert(session_id, tx);
                thread::spawn(move || {
                    session_thread(ta.clone(), ctx, rx);
                });

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

        Ok(())
    }

    fn handle_close_session(
        &mut self,
        mut stream: UnixStream,
        session_id: u32,
    ) -> anyhow::Result<()> {
        println!("Closing session with ID: {}", session_id);

        let resp = match self.sessions.get(&session_id) {
            Some(tx) => {
                let (resp_tx, resp_rx) = unbounded();
                tx.send(SessionMessage::Close { resp_tx })?;
                resp_rx.recv()?
            }
            None => {
                println!("Session {} not found", session_id);
                CAResponse::CloseSession {
                    status: ErrorKind::ItemNotFound as u32,
                }
            }
        };

        let resp_data = bincode::encode_to_vec(resp, config::standard())?;
        stream.write_all(&resp_data)?;

        Ok(())
    }

    fn handle_invoke_command(
        &mut self,
        mut stream: UnixStream,
        session_id: u32,
        cmd_id: u32,
        params: Parameters,
    ) -> anyhow::Result<()> {
        println!("Invoking command {} on session {}", cmd_id, session_id);

        let resp = match self.sessions.get(&session_id) {
            Some(tx) => {
                let (resp_tx, resp_rx) = unbounded();
                tx.send(SessionMessage::Invoke {
                    cmd_id,
                    params,
                    resp_tx,
                })?;
                resp_rx.recv()?
            }
            None => {
                println!("Session {} not found", session_id);
                CAResponse::InvokeCommand {
                    status: ErrorKind::ItemNotFound as u32,
                }
            }
        };

        let resp_data = bincode::encode_to_vec(resp, config::standard())?;
        stream.write_all(&resp_data)?;

        Ok(())
    }

    fn next_session_id(&self) -> u32 {
        self.session_id.fetch_add(1, Ordering::SeqCst)
    }
}

// Messages sent to session threads.
enum SessionMessage {
    Invoke {
        cmd_id: u32,
        params: Parameters,
        resp_tx: Sender<CAResponse>,
    },
    Close {
        resp_tx: Sender<CAResponse>,
    },
}

// Thread function to handle a TA session.
fn session_thread<T: TrustedApplication>(
    ta: Arc<T>,
    mut ctx: T::SessionContext,
    rx: Receiver<SessionMessage>,
) {
    for msg in rx.iter() {
        match msg {
            SessionMessage::Invoke {
                cmd_id,
                mut params,
                resp_tx,
            } => {
                let resp = match ta.invoke_command(cmd_id, &mut params, &mut ctx) {
                    Ok(_) => CAResponse::InvokeCommand { status: 0 },
                    Err(e) => CAResponse::InvokeCommand {
                        status: e.raw_code(),
                    },
                };
                let _ = resp_tx.send(resp);
            }
            SessionMessage::Close { resp_tx } => {
                let resp = match ta.close_session(&mut ctx) {
                    Ok(_) => CAResponse::CloseSession { status: 0 },
                    Err(e) => CAResponse::CloseSession {
                        status: e.raw_code(),
                    },
                };
                let _ = resp_tx.send(resp);
                break;
            }
        }
    }
}
