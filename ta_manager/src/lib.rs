use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

use bincode::config;
use optee_utee::Result;

use crate::protocol::{CARequest, Parameters, TARequest};

const SERVER_SOCKET_PATH: &str = "/tmp/server.sock";

pub mod protocol;

/// Trait representing a Trusted Application (TA).
pub trait TrustedApplication {
    /// Create a new TA instance.
    fn create(&self) -> Result<()>;

    /// Open a session with the TA.
    fn open_session(&self, params: &mut Parameters) -> Result<()>;

    /// Close the session with the TA.
    fn close_session(&self) -> Result<()>;

    /// Destroy the TA instance.
    fn destroy(&self) -> Result<()>;

    /// Invoke a command on the TA.
    fn invoke_command(&self, cmd_id: u32, params: &mut Parameters) -> Result<()>;
}

/// Run the Trusted Application (TA).
pub fn run_ta<T: TrustedApplication>(ta: T, uuid: &str) -> anyhow::Result<()> {
    ta.create()?;
    let _stream = register_ta(uuid)?;
    handle_ca_request(&ta, uuid)?;
    Ok(())
}

// Register the TA with the TA Manager.
fn register_ta(uuid: &str) -> anyhow::Result<UnixStream> {
    let mut stream = UnixStream::connect(SERVER_SOCKET_PATH)?;

    let req = TARequest::Register {
        uuid: uuid.to_string(),
    };
    let data = bincode::encode_to_vec(req, config::standard())?;
    stream.write_all(&data)?;
    println!("TA registered with UUID: {}", uuid);

    Ok(stream)
}

// Handle requests from the Client Application (CA).
fn handle_ca_request<T: TrustedApplication>(ta: &T, uuid: &str) -> anyhow::Result<()> {
    let path = PathBuf::from(format!("/tmp/{}.sock", uuid));
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
                ta.open_session(&mut params)?;
            }
            CARequest::CloseSession => {
                ta.close_session()?;
            }
            CARequest::Destroy => {
                ta.destroy()?;
                break;
            }
            CARequest::InvokeCommand { cmd_id, mut params } => {
                ta.invoke_command(cmd_id, &mut params)?;
            }
        }
    }

    Ok(())
}
