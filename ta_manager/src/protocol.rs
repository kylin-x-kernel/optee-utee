use bincode::{Decode, Encode};

#[derive(Encode, Decode, Debug)]
pub enum TARequest {
    Register { uuid: String },
}

#[derive(Encode, Decode)]
pub enum TeeRequest {
    OpenSession {
        uuid: String,
        connection_method: u32,
        params: Parameters,
    },
    CloseSession {
        session_id: u32,
    },
    InvokeCommand {
        session_id: u32,
        cmd_id: u32,
        params: Parameters,
    },
    RequestCancellation {
        session_id: u32,
    },
}

#[derive(Encode, Decode)]
pub enum TeeResponse {
    OpenSession { session_id: u32, result: u32 },
    CloseSession { result: u32 },
    InvokeCommand { params: Parameters, result: u32 },
    RequestCancellation { result: u32 },
}

#[derive(Encode, Decode, Default)]
pub struct Parameters(pub Parameter, pub Parameter, pub Parameter, pub Parameter);

#[derive(Encode, Decode, Default)]
pub struct Parameter {
    pub param: TeeParam,
    pub param_type: ParamType,
}

#[derive(Encode, Decode, Default)]
pub struct TeeParam {
    pub data: Vec<u8>,
    pub values: Value,
}

#[derive(Encode, Decode, Clone, Copy, Default)]
pub struct Value {
    pub a: u32,
    pub b: u32,
}

#[derive(Encode, Decode, Default, Debug, PartialEq, Eq, Clone, Copy)]
pub enum ParamType {
    #[default]
    None = 0,
    ValueInput = 1,
    ValueOutput = 2,
    ValueInout = 3,
    MemrefInput = 5,
    MemrefOutput = 6,
    MemrefInout = 7,
}

impl From<u32> for ParamType {
    fn from(value: u32) -> Self {
        match value {
            0 => ParamType::None,
            1 => ParamType::ValueInput,
            2 => ParamType::ValueOutput,
            3 => ParamType::ValueInout,
            5 => ParamType::MemrefInput,
            6 => ParamType::MemrefOutput,
            7 => ParamType::MemrefInout,
            _ => ParamType::None,
        }
    }
}
