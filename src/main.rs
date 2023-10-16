mod vtk;
use std::{
    collections::HashMap,
    error::Error,
    thread,
    time::{self, Duration, Instant},
};

use vtk::Vtk;

use crate::vtk::{Tlv, TlvKey};

fn match_sta(msg: Tlv) -> Option<u32> {
    let mut sta = false;
    let mut amount: Option<u32> = None;

    for rec in msg.data {
        if rec.0 == TlvKey::MsgName {
            let msg_name = String::from_utf8(rec.1.clone()).unwrap();
            if msg_name == "STA" {
                sta = true;
            }
        }
        if rec.0 == TlvKey::AmountInMinorCurrencyUnit {
            amount = Some(String::from_utf8(rec.1.clone()).unwrap().parse().unwrap());
        }
    }
    if sta && amount.is_some() {
        amount
    } else {
        None
    }
}

fn match_vrp(msg: Tlv, money: u32) -> bool {
    let mut vrp = false;
    let mut amount: Option<u32> = None;
    for rec in msg.data {
        if rec.0 == TlvKey::MsgName {
            let msg_name = String::from_utf8(rec.1.clone()).unwrap();
            if msg_name == "VRP" {
                vrp = true;
            }
        }
        if rec.0 == TlvKey::AmountInMinorCurrencyUnit {
            amount = Some(String::from_utf8(rec.1.clone()).unwrap().parse().unwrap());
        }
    }
    if vrp && amount.unwrap() == money {
        true
    } else {
        false
    }
}

fn match_fin(msg: Tlv, money: u32) -> bool {
    let mut fin = false;
    let mut amount: Option<u32> = None;
    for rec in msg.data {
        if rec.0 == TlvKey::MsgName {
            let msg_name = String::from_utf8(rec.1.clone()).unwrap();
            if msg_name == "FIN" {
                fin = true;
            }
        }
        if rec.0 == TlvKey::AmountInMinorCurrencyUnit {
            amount = Some(String::from_utf8(rec.1.clone()).unwrap().parse().unwrap());
        }
    }
    if fin && amount.unwrap() == money {
        true
    } else {
        false
    }
}

fn main() {
    let mut dev = vtk::Vtk::new("/dev/ttyUSB0").unwrap();
    let mut timer = time::Instant::now();

    let mut money = 0;
    let mut sta = false;
    let mut vrp = false;
    let mut fin = false;

    loop {
        
        if timer.elapsed() >= Duration::from_secs(270) {
            let mut tlv = Tlv::new();
            tlv.set_str(TlvKey::OperationNum, &dev.operation_num.to_string().clone());
            dev.send("IDL", tlv.clone()).unwrap();
            timer = Instant::now();
        }

        if let Ok(msg) = dev.receive() {
            println!("msg: {:?}", msg);
            let res = match_sta(msg.clone());
            if res.is_some() {
                dev.send_vrp(res.unwrap());
                sta = true;
                money = res.unwrap();
            }

            if sta {
                let res = match_vrp(msg.clone(), money);
                if res {
                    dev.send_fin(money);
                    vrp = true;
                }
            }

            if vrp {
                let res = match_fin(msg.clone(), money);
                if res {
                    fin = true;
                }
            }
        };

        if sta && vrp && fin {
            println!("Средства: {}, начислены!", money);
            dev.idle(None).unwrap();
            sta = false;
            vrp = false;
            fin = false;
            money = 0;
        }
        thread::sleep(Duration::from_secs(1));
    }
}
