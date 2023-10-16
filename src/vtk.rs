use core::str;
use std::{
    collections::HashMap,
    io::{Error, Read, Write},
    time::Duration,
};

use num_derive::FromPrimitive;
use serialport::SerialPort;

const READ_TIMEOUT: Duration = Duration::from_millis(2000);

#[derive(PartialEq, Hash, Eq, FromPrimitive, Debug, Clone, Copy)]
#[repr(u8)]
pub enum TlvKey {
    MsgName = 0x01,
    OperationNum = 0x03,
    AmountInMinorCurrencyUnit = 0x04,
    KeepaliveIntervalInSecs = 0x05,
    OperationTimeoutInSecs = 0x06,
    EventName = 0x07,
    EventNum = 0x08,
    ProductId = 0x09,
    QrCodeData = 0x0A,
    TcpIpDestantion = 0x0B,
    OutgoingByteCounter = 0x0C,
    SimpleDataBlock = 0x0D,
    ConfirmableDataBlock = 0x0E,
    ProductName = 0x0F,
    PosManagementData = 0x10,
    LocalTime = 0x11,
    SysInfo = 0x12,
    BankingReceipt = 0x13,
    DisplayTimeInMs = 0x14,
}

#[derive(Clone, Debug)]
pub struct Tlv {
    pub data: HashMap<TlvKey, Vec<u8>>,
}

impl Tlv {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    fn deser_one(raw: &Vec<u8>, begin: usize) -> Option<(u8, Vec<u8>, usize)> {
        if raw.len() - begin < 2 {
            return None;
        }
        let k = raw[begin];
        let len = raw[begin + 1] as usize;
        if (begin + len + 2) > raw.len() {
            return None;
        }
        let v = raw[begin + 2..begin + len + 2].to_vec();

        Some((k, v, len + 2))
    }

    pub fn deserialize(raw: &Vec<u8>) -> Self {
        let mut data = HashMap::new();
        let mut i = 0;
        loop {
            match Self::deser_one(raw, i) {
                Some((k, v, len)) => {
                    match num::FromPrimitive::from_u8(k) {
                        Some(k) => {
                            let key: TlvKey = k;
                            if data.get(&key).is_none() {
                                data.insert(k, v);
                            }
                        }
                        None => (),
                    }
                    i += len;
                }
                None => break,
            }
        }
        Self { data: data }
    }

    pub fn serialize(self) -> Vec<u8> {
        let mut output = Vec::new();
        for (k, v) in self.data {
            output.push(k as u8);
            let len = v.len() as u8;
            output.push(len);
            for b in v {
                output.push(b);
            }
        }
        output
    }

    pub fn data<'a>(&'a self) -> &'a HashMap<TlvKey, Vec<u8>> {
        &self.data
    }

    pub fn get_bin(&self, key: TlvKey) -> Option<&Vec<u8>> {
        self.data.get(&key)
    }

    pub fn set_bin(&mut self, key: TlvKey, data: &[u8]) {
        self.data.insert(key, data.to_vec());
    }

    pub fn set_str(&mut self, key: TlvKey, data: &str) {
        self.data.insert(key, data.as_bytes().to_vec());
    }
}

pub struct Vtk {
    pub operation_num: u32,
    pub port: Box<dyn SerialPort>,
}

impl Vtk {
    pub fn new(driver: &str) -> Result<Self, Error> {
        let mut port = serialport::new(driver, 115200)
            .timeout(READ_TIMEOUT)
            .parity(serialport::Parity::None)
            .flow_control(serialport::FlowControl::None)
            .open()?;

        let mut oper_num: u32 = 0;
        let mut tlv = Tlv::new();
        tlv.set_str(TlvKey::MsgName, "IDL");
        let mut tlv = tlv.serialize();
        let mut buf = Vec::new();
        buf.push(0x1F);
        let len = (tlv.len() + 2) as u16;
        let len_buf: [u8; 2] = len.to_be_bytes();
        buf.push(len_buf[0]);
        buf.push(len_buf[1]);
        buf.push(0x96);
        buf.push(0xFB);
        buf.append(&mut tlv);
        let crc = get_crc(buf.clone()).to_be_bytes();
        buf.push(crc[0]);
        buf.push(crc[1]);

        port.write_all(&buf)?;

        let mut buf: [u8; 512] = [0; 512];
        let size = port.read(&mut buf)?;
        if size < 9 {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                "too few bytes received",
            ));
        }
        let responce = Tlv::deserialize(&buf[5..].to_vec());

        for rec in responce.data {
            if rec.0 == TlvKey::OperationNum {
                oper_num = String::from_utf8(rec.1.clone()).unwrap().parse().unwrap();
            }
        }

        Ok(Self {
            port: {
                serialport::new(driver, 115200)
                    .timeout(READ_TIMEOUT)
                    .parity(serialport::Parity::None)
                    .flow_control(serialport::FlowControl::None)
                    .open()?
            },
            operation_num: oper_num,
        })
    }

    pub fn send_vrp(&mut self, amount: u32) {
        self.operation_num += 1;
        let mut tlv = Tlv::new();
        tlv.set_str(TlvKey::AmountInMinorCurrencyUnit, &amount.to_string());
        tlv.set_str(TlvKey::OperationNum, &self.operation_num.to_string());
        self.send("VRP", tlv.clone()).unwrap();
    }

    pub fn send_fin(&mut self, amount: u32) {
        let mut tlv = Tlv::new();
        tlv.set_str(TlvKey::AmountInMinorCurrencyUnit, &amount.to_string());
        tlv.set_str(TlvKey::OperationNum, &self.operation_num.to_string());
        self.send("FIN", tlv.clone()).unwrap();
    }

    pub fn idle(&mut self, add: Option<Tlv>) -> Result<(), Error> {
        let tlv = match add {
            Some(tlv) => tlv,
            None => Tlv::new(),
        };
        self.send("IDL", tlv)?;
        Ok(())
    }

    pub fn disable(&mut self, add: Tlv) -> Result<(), Error> {
        self.send("DIS", Tlv::new())?;
        _ = self.receive()?;
        Ok(())
    }

    pub fn show_qr(&mut self, qr: &str) -> Result<(), Error> {
        let mut tlv = Tlv::new();
        tlv.set_str(TlvKey::QrCodeData, qr);
        self.idle(Some(tlv))
    }

    pub fn send(&mut self, msg_name: &str, mut tlv: Tlv) -> Result<(), Error> {
        tlv.set_str(TlvKey::MsgName, msg_name);
        let mut tlv = tlv.serialize();
        let mut buf = Vec::new();
        buf.push(0x1F);
        let len = (tlv.len() + 2) as u16;
        let len_buf: [u8; 2] = len.to_be_bytes();
        buf.push(len_buf[0]);
        buf.push(len_buf[1]);
        buf.push(0x96);
        buf.push(0xFB);
        buf.append(&mut tlv);
        let crc = get_crc(buf.clone()).to_be_bytes();
        buf.push(crc[0]);
        buf.push(crc[1]);

        self.port.write_all(&buf)
    }

    pub fn receive(&mut self) -> Result<Tlv, Error> {
        let mut buf: [u8; 512] = [0; 512];
        let size = self.port.read(&mut buf)?;
        if size < 9 {
            return Err(Error::new(
                std::io::ErrorKind::Other,
                "too few bytes received",
            ));
        }
        Ok(Tlv::deserialize(&buf[5..].to_vec()))
    }
}

const CRC16_CCITT_TABLE: [u16; 256] = [
    0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50A5, 0x60C6, 0x70E7, 0x8108, 0x9129, 0xA14A, 0xB16B,
    0xC18C, 0xD1AD, 0xE1CE, 0xF1EF, 0x1231, 0x0210, 0x3273, 0x2252, 0x52B5, 0x4294, 0x72F7, 0x62D6,
    0x9339, 0x8318, 0xB37B, 0xA35A, 0xD3BD, 0xC39C, 0xF3FF, 0xE3DE, 0x2462, 0x3443, 0x0420, 0x1401,
    0x64E6, 0x74C7, 0x44A4, 0x5485, 0xA56A, 0xB54B, 0x8528, 0x9509, 0xE5EE, 0xF5CF, 0xC5AC, 0xD58D,
    0x3653, 0x2672, 0x1611, 0x0630, 0x76D7, 0x66F6, 0x5695, 0x46B4, 0xB75B, 0xA77A, 0x9719, 0x8738,
    0xF7DF, 0xE7FE, 0xD79D, 0xC7BC, 0x48C4, 0x58E5, 0x6886, 0x78A7, 0x0840, 0x1861, 0x2802, 0x3823,
    0xC9CC, 0xD9ED, 0xE98E, 0xF9AF, 0x8948, 0x9969, 0xA90A, 0xB92B, 0x5AF5, 0x4AD4, 0x7AB7, 0x6A96,
    0x1A71, 0x0A50, 0x3A33, 0x2A12, 0xDBFD, 0xCBDC, 0xFBBF, 0xEB9E, 0x9B79, 0x8B58, 0xBB3B, 0xAB1A,
    0x6CA6, 0x7C87, 0x4CE4, 0x5CC5, 0x2C22, 0x3C03, 0x0C60, 0x1C41, 0xEDAE, 0xFD8F, 0xCDEC, 0xDDCD,
    0xAD2A, 0xBD0B, 0x8D68, 0x9D49, 0x7E97, 0x6EB6, 0x5ED5, 0x4EF4, 0x3E13, 0x2E32, 0x1E51, 0x0E70,
    0xFF9F, 0xEFBE, 0xDFDD, 0xCFFC, 0xBF1B, 0xAF3A, 0x9F59, 0x8F78, 0x9188, 0x81A9, 0xB1CA, 0xA1EB,
    0xD10C, 0xC12D, 0xF14E, 0xE16F, 0x1080, 0x00A1, 0x30C2, 0x20E3, 0x5004, 0x4025, 0x7046, 0x6067,
    0x83B9, 0x9398, 0xA3FB, 0xB3DA, 0xC33D, 0xD31C, 0xE37F, 0xF35E, 0x02B1, 0x1290, 0x22F3, 0x32D2,
    0x4235, 0x5214, 0x6277, 0x7256, 0xB5EA, 0xA5CB, 0x95A8, 0x8589, 0xF56E, 0xE54F, 0xD52C, 0xC50D,
    0x34E2, 0x24C3, 0x14A0, 0x0481, 0x7466, 0x6447, 0x5424, 0x4405, 0xA7DB, 0xB7FA, 0x8799, 0x97B8,
    0xE75F, 0xF77E, 0xC71D, 0xD73C, 0x26D3, 0x36F2, 0x0691, 0x16B0, 0x6657, 0x7676, 0x4615, 0x5634,
    0xD94C, 0xC96D, 0xF90E, 0xE92F, 0x99C8, 0x89E9, 0xB98A, 0xA9AB, 0x5844, 0x4865, 0x7806, 0x6827,
    0x18C0, 0x08E1, 0x3882, 0x28A3, 0xCB7D, 0xDB5C, 0xEB3F, 0xFB1E, 0x8BF9, 0x9BD8, 0xABBB, 0xBB9A,
    0x4A75, 0x5A54, 0x6A37, 0x7A16, 0x0AF1, 0x1AD0, 0x2AB3, 0x3A92, 0xFD2E, 0xED0F, 0xDD6C, 0xCD4D,
    0xBDAA, 0xAD8B, 0x9DE8, 0x8DC9, 0x7C26, 0x6C07, 0x5C64, 0x4C45, 0x3CA2, 0x2C83, 0x1CE0, 0x0CC1,
    0xEF1F, 0xFF3E, 0xCF5D, 0xDF7C, 0xAF9B, 0xBFBA, 0x8FD9, 0x9FF8, 0x6E17, 0x7E36, 0x4E55, 0x5E74,
    0x2E93, 0x3EB2, 0x0ED1, 0x1EF0,
];

pub fn get_crc(data: Vec<u8>) -> u16 {
    let mut crc: u16 = 0xffff;

    for i in 0..data.len() {
        let tmp = (crc >> 8) ^ (0x00ff & data[i] as u16);
        crc = (crc << 8) ^ CRC16_CCITT_TABLE[tmp as usize]
    }
    crc
}
