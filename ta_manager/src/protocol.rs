use bincode::{Decode, Encode};

#[derive(Encode, Decode)]
pub enum TARequest {
    Register { uuid: String },
}

#[derive(Encode, Decode)]
pub enum CARequest {
    OpenSession { params: Parameters },
    CloseSession,
    Destroy,
    InvokeCommand { cmd_id: u32, params: Parameters },
}

#[derive(Encode, Decode)]
pub struct Parameters(pub Parameter, pub Parameter, pub Parameter, pub Parameter);

#[derive(Encode, Decode)]
pub struct Parameter {
    pub raw: TEEParam,
    pub param_type: ParamType,
}

#[derive(Encode, Decode)]
pub struct TEEParam {
    pub data: Vec<u8>,
    pub value: Value,
}

#[derive(Encode, Decode, Clone, Copy)]
pub struct Value {
    pub a: u32,
    pub b: u32,
}

#[derive(Encode, Decode)]
pub enum ParamType {
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
